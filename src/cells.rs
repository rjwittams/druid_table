use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{Affine, BoxConstraints, Command, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Size, Target, UpdateCtx, Widget, WidgetPod, Selector};

use crate::axis_measure::{
    AxisMeasure, AxisMeasureAdjustment, AxisPair, LogIdx, TableAxis, VisIdx, VisOffset,
    ADJUST_AXIS_MEASURE,
};
use crate::cells::Editing::Inactive;
use crate::columns::{CellCtx, CellRender};
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{IndexedData, RemapSpec, Remapper, SortSpec};
use crate::headings::{HeaderAction, HeaderActionType, HEADER_CLICKED};
use crate::render_ext::RenderContextExt;
use crate::selection::{CellDemap, CellRect, SingleCell, SingleSlice, TableSelection};
use crate::table::TableState;
use crate::{EditorFactory, IndexedItems, Remap, SortDirection};
use druid::widget::Bindable;

pub trait CellsDelegate<TableData: IndexedData>:
    CellRender<TableData::Item> + Remapper<TableData> + EditorFactory<TableData::Item>
where
    TableData::Item: Data,
{
    fn number_of_columns_in_data(&self, data: &TableData) -> usize;
}

enum Editing<RowData> {
    Inactive,
    Cell {
        single_cell: SingleCell,
        child: WidgetPod<RowData, Box<dyn Widget<RowData>>>,
    },
}

impl<RowData: Data> Editing<RowData> {
    fn is_active(&self) -> bool {
        match self {
            Inactive => false,
            _ => true,
        }
    }

    fn is_editing(&self, cell:&SingleCell)->bool{
        match self {
            Editing::Cell { single_cell, .. } => {
                single_cell.vis.eq(&cell.vis)
            }
            _ => false,
        }
    }

    fn handle_event<TableData: IndexedItems<Idx=LogIdx, Item=RowData>>(&mut self, ctx: &mut EventCtx,
                                                           event: &Event,
                                                           data: &mut TableData,
                                                           env: &Env){
        match self {
            Editing::Cell { ref single_cell, ref mut child } => {
                data.with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
            }
            _ => { },
        }
    }

    fn start_editing<TableData: IndexedItems<Item=RowData>>(&mut self,
                                                            ctx: &mut EventCtx,
                                                            data: &mut TableData,
                                                            cell:&SingleCell,
                                                            make_editor: impl FnMut(&CellCtx)->Option<Box<dyn Widget<RowData>>> ){
        self.stop_editing(data);
        let mut me = make_editor;
        let cell_ctx = CellCtx::Cell(&cell);
        if let Some(editor) = me(&cell_ctx) {

            let pod = WidgetPod::new(editor);

            log::info!("Made editor widget id p {:?} c{:?}", ctx.widget_id(),pod.id());
            *self = Editing::Cell {
                single_cell: cell.clone(),
                child: pod,
            };

            ctx.children_changed();
            ctx.request_layout();
            ctx.set_handled();
        }
    }

    fn stop_editing<TableData: IndexedItems<Item=RowData>>(&mut self, data: &mut TableData) {
        match self{
            Editing::Cell {single_cell, child}=>{
                // Work out what to do with the previous pod if there is one.
                // We could have lazy editors (that don't write back to data immediately) and send them a special command saying 'you are being shut down'.
                // Would need to give them data for their row
            }
            Editing::Inactive=>{}
        }
        *self = Editing::Inactive
    }
}

pub struct Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    CellDel: CellsDelegate<TableData>, // The length is the number of columns
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    cell_delegate: CellDel,
    editing: Editing<RowData>,
    dragging_selection: bool,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

impl<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
    Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    CellDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    pub fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        row_measure: RowMeasure,
        cells_delegate: CellDel,
    ) -> Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure> {
        Cells {
            config,
            resolved_config: None,
            column_measure,
            row_measure,
            cell_delegate: cells_delegate,
            editing: Inactive,
            dragging_selection: false,
            phantom_rd: PhantomData::default(),
            phantom_td: PhantomData::default(),
        }
    }

    fn find_cell(&self, data: &TableState<TableData>, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            self.row_measure.vis_idx_from_pixel(pos.y),
            self.column_measure.vis_idx_from_pixel(pos.x),
        );
        let log_row = r.and_then(|r| data.remaps[&TableAxis::Rows].get_log_idx(r));
        let log_col = c.and_then(|c| data.remaps[&TableAxis::Columns].get_log_idx(c));
        Some(SingleCell::new(
            AxisPair::new(r?, c?),
            AxisPair::new(log_row?, log_col?),
        ))
    }

    fn measured_size(&mut self) -> Size {
        Size::new(
            self.column_measure.total_pixel_length(),
            self.row_measure.total_pixel_length(),
        )
    }

    fn paint_cells(
        &self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        env: &Env,
        rect: &CellRect,
    ) {
        for vis_row_idx in rect.rows() {
            let row_top = self.row_measure.first_pixel_from_vis(vis_row_idx);
            if let Some(log_row_idx) = data.remaps[&TableAxis::Rows].get_log_idx(vis_row_idx) {
                data.data.with(log_row_idx, |row| {
                    self.paint_row(
                        ctx,
                        env,
                        &mut rect.cols(),
                        log_row_idx,
                        vis_row_idx,
                        row_top,
                        row,
                    )
                });
            }
        }
    }

    fn paint_row(
        &self,
        ctx: &mut PaintCtx,
        env: &Env,
        cols: &mut impl Iterator<Item = VisIdx>,
        log_row_idx: LogIdx,
        vis_row_idx: VisIdx,
        row_top: Option<f64>,
        row: &RowData,
    ) {
        if let Some(rtc) = &self.resolved_config {
            for vis_col_idx in cols {
                if let Some(log_col_idx) = Remap::Pristine.get_log_idx(vis_col_idx) {
                    let cell_left = self.column_measure.first_pixel_from_vis(vis_col_idx);

                    let cell_rect = Rect::from_origin_size(
                        Point::new(cell_left.unwrap_or(0.), row_top.unwrap_or(0.)),
                        Size::new(
                            self.column_measure
                                .pixels_length_for_vis(vis_col_idx)
                                .unwrap_or(0.),
                            self.row_measure
                                .pixels_length_for_vis(vis_row_idx)
                                .unwrap_or(0.),
                        ),
                    );
                    let padded_rect = cell_rect.inset(-rtc.cell_padding);

                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));

                        let sc = SingleCell::new(
                            AxisPair::new(vis_row_idx, vis_col_idx),
                            AxisPair::new(log_row_idx, log_col_idx),
                        );
                        let cell = CellCtx::Cell(&sc);
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            self.cell_delegate.paint(ctxt, &cell, row, env);
                        });
                    });

                    ctx.stroke_bottom_left_border(
                        &cell_rect,
                        &rtc.cells_border,
                        rtc.cell_border_thickness,
                    );
                } else {
                    log::warn!("Could not find logical column for {:?}", vis_col_idx)
                }
            }
        }
    }

    fn paint_selections(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        rtc: &ResolvedTableConfig,
        cell_rect: &CellRect,
    ) {
        let selected = data.selection.get_drawable_selections(cell_rect);

        let (max_x, max_y) = (
            self.column_measure.total_pixel_length(),
            self.row_measure.total_pixel_length(),
        );
        let sel_color = &rtc.selection_color;
        let sel_fill = &sel_color.clone().with_alpha(0.2);
        for range_rect in &selected.ranges {
            let fetched = (
                self.column_measure
                    .first_pixel_from_vis(range_rect.start_col),
                self.column_measure
                    .far_pixel_from_vis(range_rect.end_col)
                    .unwrap_or(max_x),
                self.row_measure.first_pixel_from_vis(range_rect.start_row),
                self.row_measure
                    .far_pixel_from_vis(range_rect.end_row)
                    .unwrap_or(max_y),
            );
            if let (Some(x0), x1, Some(y0), y1) = fetched {
                let range_draw_rect = Rect::new(x0, y0, x1, y1);
                ctx.fill(range_draw_rect, sel_fill);
                ctx.stroke(range_draw_rect, sel_color, rtc.cell_border_thickness)
            }
        }

        if let Some(focus) = selected.focus {
            let fetched = (
                self.column_measure.first_pixel_from_vis(focus.col),
                self.column_measure
                    .far_pixel_from_vis(focus.col)
                    .unwrap_or(max_x),
                self.row_measure.first_pixel_from_vis(focus.row),
                self.row_measure
                    .far_pixel_from_vis(focus.row)
                    .unwrap_or(max_y),
            );
            if let (Some(x0), x1, Some(y0), y1) = fetched {
                let range_draw_rect = Rect::new(x0, y0, x1, y1);
                ctx.stroke(
                    range_draw_rect,
                    &rtc.focus_color,
                    (rtc.cell_border_thickness * 1.5).min(2.),
                )
            }
        }
    }


    fn paint_editing(&mut self, ctx: &mut PaintCtx, data: &TableState<TableData>, env: &Env) {
        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                let vis = &single_cell.vis;

                let size = Size::new(
                    self.column_measure
                        .pixels_length_for_vis(vis.col)
                        .unwrap_or(0.),
                    self.row_measure
                        .pixels_length_for_vis(vis.row)
                        .unwrap_or(0.),
                );
                let origin = Point::new(
                    self.column_measure
                        .first_pixel_from_vis(vis.col)
                        .unwrap_or(0.),
                    self.row_measure.first_pixel_from_vis(vis.row).unwrap_or(0.),
                );

                ctx.with_save(|ctx| {
                    ctx.render_ctx.clip(Rect::from_origin_size(origin, size));
                    data.data
                        .with(single_cell.log.row, |row| child.paint(ctx, row, env));
                });
            }
            _ => (),
        }
    }

}

pub const INIT_CELLS: Selector<()> = Selector::new("druid-builtin.table.init-cells");

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> Widget<TableState<TableData>>
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableState<TableData>,
        env: &Env,
    ) {
        if let Some(rtc) = &self.resolved_config {
            let mut new_selection: Option<TableSelection> = None;
            let mut remap_changed = AxisPair::new(false, false);

            match event {
                Event::MouseDown(me) => {
                    if let Some(cell) = self.find_cell(data, &me.pos) {
                        if self.editing.is_editing(&cell) {
                            self.editing.handle_event(ctx, event, &mut data.data, env);
                        }else{
                            if me.count == 1 {
                                if me.mods.meta() || me.mods.ctrl() {
                                    new_selection = data.selection.add_selection(cell.into());
                                } else if me.mods.shift() {
                                    new_selection = data.selection.move_extent(cell.into());
                                } else {
                                    new_selection = Some(cell.into());
                                }

                                ctx.set_handled();
                                self.editing.stop_editing(&mut data.data);
                                self.dragging_selection = true;
                                ctx.set_active(true);
                            } else if me.count == 2 {
                                let cd = &mut self.cell_delegate;
                                self.editing.start_editing(ctx,&mut data.data, &cell,  |cell_ctx| cd.make_editor(cell_ctx));
                            }
                        }
                    }
                }
                Event::MouseMove(me) if !self.editing.is_active() && self.dragging_selection =>{
                    if let Some(cell) = self.find_cell(data, &me.pos) {
                        new_selection = data.selection.move_extent(cell.into());
                    }
                },
                Event::MouseUp(me) if self.dragging_selection =>{
                    self.dragging_selection = false;
                    ctx.set_active(false);
                }
                Event::Command(cmd) => {
                    if let Some(_)  = cmd.get(INIT_CELLS) {
                        data.remap_specs[&TableAxis::Rows] = self.cell_delegate.initial_spec();
                        remap_changed[&TableAxis::Rows] = true;
                        remap_changed[&TableAxis::Columns] = true;
                    } else if let Some(AxisMeasureAdjustment::LengthChanged(axis, idx, length)) = cmd.get(ADJUST_AXIS_MEASURE)
                    {
                        match axis {
                            TableAxis::Rows => {
                                // If we share the measure through Rc Refcell, we don't need to update it
                                if !self.row_measure.shared() {
                                    self.row_measure.set_pixel_length_for_vis(*idx, *length);
                                }
                            }
                            TableAxis::Columns => {
                                if !self.column_measure.shared() {
                                    self.column_measure.set_pixel_length_for_vis(*idx, *length);
                                }
                            }
                        };
                        ctx.request_layout();
                    } else if let Some(HeaderAction(axis, vis, action)) = cmd.get(HEADER_CLICKED) {
                        let vis_addr = AxisPair::new_for_axis(axis, *vis, Default::default());
                        if let Some(log_addr) = data.remaps.get_log_cell(&vis_addr) {
                            match action {
                                HeaderActionType::ToggleSort { extend } => {
                                    remap_changed[&axis] = data.remap_specs[&axis].toggle_sort(log_addr[axis], *extend);
                                }
                            }
                        }
                    } else {
                        match &mut self.editing {
                            Editing::Cell { single_cell, child } => {
                                data.data.with_mut(single_cell.log.row, |row| {
                                    child.event(ctx, event, row, env)
                                });
                            }
                            _ => (),
                        }
                    }
                }
                Event::KeyDown(ke) if !self.editing.is_active() => {
                    match &ke.key {
                        KbKey::ArrowDown => {
                            new_selection =
                                data.selection
                                    .move_focus(&TableAxis::Rows, VisOffset(1), &data.remaps);
                            ctx.set_handled();
                        }
                        KbKey::ArrowUp => {
                            new_selection =
                                data.selection
                                    .move_focus(&TableAxis::Rows, VisOffset(-1), &data.remaps);
                            ctx.set_handled();
                        }
                        KbKey::ArrowRight => {
                            new_selection =
                                data.selection
                                    .move_focus(&TableAxis::Columns, VisOffset(1), &data.remaps);
                            ctx.set_handled();
                        }
                        KbKey::ArrowLeft => {
                            new_selection =
                                data.selection
                                    .move_focus(&TableAxis::Columns, VisOffset(-1), &data.remaps);
                            ctx.set_handled();
                        }
                        KbKey::Character(s) if s == " " => {
                            // This is to match Excel
                            if ke.mods.meta() || ke.mods.ctrl() {
                                new_selection = data.selection.extend_from_focus_in_axis(&TableAxis::Columns, &data.remaps);
                                ctx.set_handled();
                            } else if ke.mods.shift() {
                                new_selection = data.selection.extend_from_focus_in_axis(&TableAxis::Rows, &data.remaps);
                                ctx.set_handled();
                            }

                            // TODO - when Ctrl + Shift, select full grid
                        },
                        KbKey::Copy =>{
                            log::info!("Copy")

                        },
                        k => {
                            log::info!("Key {:?}" , k )
                        },
                    }
                }
                _ => match &mut self.editing {
                    Editing::Cell { single_cell, child } => {
                        data.data
                            .with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
                    }
                    _ => (),
                },
            }

            if let Some(sel) = new_selection {
                data.selection = sel;
                if data.selection.has_focus() && !self.editing.is_active() {
                    ctx.request_focus();
                }
            }

            // TODO: move to update but need versioned pointers on measures
            if remap_changed[&TableAxis::Rows] {
                data.remap_axis(&TableAxis::Rows, |d, s| self.cell_delegate.remap_items(d, s));
                self.row_measure.set_axis_properties(
                    rtc.cell_border_thickness,
                    data.data.idx_len(),
                    &data.remaps[&TableAxis::Rows],
                );
                ctx.request_layout(); // Could avoid if we know we overflow scroll?
            }
            if remap_changed[&TableAxis::Columns] {
                self.column_measure.set_axis_properties(
                    rtc.cell_border_thickness,
                    self.cell_delegate.number_of_columns_in_data(&data.data),
                    &data.remaps[&TableAxis::Columns],
                );
                ctx.request_layout();
            }
            // Todo remap cols
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<TableData>,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            self.resolved_config = Some(self.config.resolve(env));
            ctx.submit_command(INIT_CELLS.with(()), ctx.widget_id() );
        } else {
            match &mut self.editing {
                Editing::Cell { single_cell, child } => {
                    log::info!("LC event {:?}", event);
                    data.data.with(single_cell.log.row, |row| {
                        child.lifecycle(ctx, event, row, env)
                    });
                }
                _ => (),
            }
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &TableState<TableData>,
        data: &TableState<TableData>,
        _env: &Env,
    ) {

        if !old_data.data.same(&data.data) || !old_data.remap_specs.same(&data.remap_specs) {
            // TODO send self a message to sort
        }
        if !old_data.selection.same(&data.selection){
            ctx.request_paint();
        }
        //TODO Columns update from data
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &TableState<TableData>,
        env: &Env,
    ) -> Size {
        bc.debug_check("TableCells");

        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                let vis = &single_cell.vis;
                let size = Size::new(
                    self.column_measure
                        .pixels_length_for_vis(vis.col)
                        .unwrap_or(0.),
                    self.row_measure
                        .pixels_length_for_vis(vis.row)
                        .unwrap_or(0.),
                );
                let origin = Point::new(
                    self.column_measure
                        .first_pixel_from_vis(vis.col)
                        .unwrap_or(0.),
                    self.row_measure.first_pixel_from_vis(vis.row).unwrap_or(0.),
                );
                let bc = BoxConstraints::tight(size);
                data.data.with(single_cell.log.row, |row| {
                    let size = child.layout(ctx, &bc, row, env);
                    child.set_layout_rect(ctx, row, env, Rect::from_origin_size(origin, size))
                });
            }
            _ => (),
        }
        let measured = self.measured_size();
        let size =  bc.constrain(measured);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableState<TableData>, env: &Env) {
        self.cell_delegate.init(ctx, env); // TODO reduce calls? Invalidate on some changes

        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        let draw_rect = rect.intersect(Rect::from_origin_size(Point::ZERO, self.measured_size()));

        ctx.fill(draw_rect, &rtc.cells_background);

        let cell_rect = CellRect::new(
            self.row_measure
                .vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            self.column_measure
                .vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );

        self.paint_cells(ctx, data, env, &cell_rect);
        self.paint_selections(ctx, data, &rtc, &cell_rect);

        self.paint_editing(ctx, data, env)
    }
}


impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> Bindable
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
}


