use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Command, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, Size, Target, UpdateCtx, Widget, WidgetPod,
};

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
use crate::selection::{
    CellDemap, CellRect, SingleCell, SingleSlice, TableSelection,
};
use crate::{EditorFactory, IndexedItems, Remap, SortDirection};
use druid::widget::Bindable;
use crate::table::TableState;

pub trait CellsDelegate<TableData: IndexedData>:
    CellRender<TableData::Item> + Remapper<TableData> + EditorFactory<TableData::Item>
where
    TableData::Item: Data,
{
    fn number_of_columns_in_data(&self, data: &TableData) -> usize;
}

#[derive(Clone)]
pub struct RemapChanged(pub TableAxis, pub RemapSpec, pub Option<Remap>);

pub enum TableChange {
    Selection(TableSelection),
    Remap(RemapChanged),
}

pub enum SubmitCommand<'a, 'b, 'c> {
    E(&'a mut EventCtx<'b, 'c>),
    L(&'a mut LifeCycleCtx<'b, 'c>),
}

impl SubmitCommand<'_, '_, '_> {
    pub fn submit_command(&mut self, cmd: impl Into<Command>, target: impl Into<Option<Target>>) {
        match self {
            Self::E(ctx) => ctx.submit_command(cmd, target),
            Self::L(ctx) => ctx.submit_command(cmd, target),
        }
    }
}

pub type TableChangedHandler = dyn Fn(&mut SubmitCommand, &TableChange);

enum Editing<RowData> {
    Inactive,
    Cell {
        single_cell: SingleCell,
        child: WidgetPod<RowData, Box<dyn Widget<RowData>>>,
    },
}

impl<RowData> Editing<RowData> {
    fn is_active(&self) -> bool {
        match self {
            Inactive => false,
            _ => true,
        }
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
    selection: TableSelection,
    change_handlers: Vec<Box<TableChangedHandler>>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    cell_delegate: CellDel,
    remap_spec_rows: RemapSpec,
    remap_rows: Remap,
    editing: Editing<RowData>,
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
            change_handlers: Vec::new(),
            selection: TableSelection::NoSelection,
            column_measure,
            row_measure,
            remap_spec_rows: RemapSpec::default(),
            remap_rows: Remap::Pristine,
            cell_delegate: cells_delegate,
            editing: Inactive,
            phantom_rd: PhantomData::default(),
            phantom_td: PhantomData::default(),
        }
    }

    pub fn add_change_handlers(
        &mut self,
        selection_handler: impl Fn(&mut SubmitCommand, &TableChange) + 'static,
    ) {
        self.change_handlers.push(Box::new(selection_handler));
    }

    fn set_selection(&mut self, ctx: &mut EventCtx, selection: TableSelection) {
        self.selection = selection;
        let tc = TableChange::Selection(self.selection.clone());
        self.call_change_handlers(SubmitCommand::E(ctx), &tc);
        ctx.request_paint();
    }

    fn call_change_handlers(&mut self, sc: SubmitCommand, tc: &TableChange) {
        let mut sc_ = sc;
        for sh in &self.change_handlers {
            sh(&mut sc_, tc)
        }
    }

    fn find_cell(&self, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            self.row_measure.vis_idx_from_pixel(pos.y),
            self.column_measure.vis_idx_from_pixel(pos.x),
        );
        let log_row = r.and_then(|r| self.remap_rows.get_log_idx(r));
        let log_col = c.and_then(|c| Remap::Pristine.get_log_idx(c)); // TODO! Moving columns
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
        data: &impl IndexedItems<Item = RowData, Idx = LogIdx>,
        env: &Env,
        rect: &CellRect,
    ) {
        for vis_row_idx in rect.rows() {
            let row_top = self.row_measure.first_pixel_from_vis(vis_row_idx);
            if let Some(log_row_idx) = self.remap_rows.get_log_idx(vis_row_idx) {
                data.with(log_row_idx, |row| {
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
        rtc: &ResolvedTableConfig,
        cell_rect: &CellRect,
    ) {
        let selected = self.selection.get_drawable_selections(cell_rect);

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

    fn remap_for_axis(&self, axis: TableAxis) -> &Remap {
        match axis {
            TableAxis::Rows => &self.remap_rows,
            TableAxis::Columns => &Remap::Pristine,
        }
    }
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> Widget<TableState<TableData>>
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut TableState<TableData>, env: &Env) {
        let mut new_selection: Option<TableSelection> = None;

        match event {
            Event::MouseDown(me) => {
                let ed_cell = match &mut self.editing {
                    Editing::Cell { single_cell, child } => {
                        data.data.with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
                        Some(single_cell.vis.clone())
                    }
                    _ => None,
                };

                let found_cell = self.find_cell(&me.pos);
                if !ctx.is_handled() {
                    if let Some(cell) = found_cell {
                        if Some(cell.vis.clone()) != ed_cell {
                            if me.count == 1 {
                                new_selection = Some(cell.into());
                                // TODO: Modifier keys ask current selection to add this cell
                                // TODO: deactivate any editor
                                ctx.set_handled();
                                self.editing = Editing::Inactive;
                            } else if me.count == 2 {
                                let cell_ctx = CellCtx::Cell(&cell);
                                if let Some(editor) = self.cell_delegate.make_editor(&cell_ctx) {
                                    let pod = WidgetPod::new(editor);
                                    // Work out what to do with the previous pod if there is one

                                    log::info!(
                                        "Made editor widget id p {:?} c{:?}",
                                        ctx.widget_id(),
                                        pod.id()
                                    );
                                    self.editing = Editing::Cell {
                                        single_cell: cell,
                                        child: pod,
                                    };

                                    ctx.children_changed();
                                    ctx.request_layout();
                                    ctx.set_handled();
                                }
                            }
                        }
                    }
                }
            }
            Event::Command(cmd) => {
                if let Some(AxisMeasureAdjustment::LengthChanged(axis, idx, length)) =
                    cmd.get(ADJUST_AXIS_MEASURE)
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
                    if let Some(log_addr) = self.get_log_cell(&vis_addr) {
                        match action {
                            HeaderActionType::Select => {
                                new_selection = Some(TableSelection::SingleSlice(
                                    SingleSlice::new(*axis, SingleCell::new(vis_addr, log_addr)),
                                ));
                            }
                            HeaderActionType::ToggleSort { extend } => {
                                // TODO: centralise remapping etc
                                let sort_by = &mut self.remap_spec_rows.sort_by;
                                let log_idx = log_addr[axis].0;

                                match sort_by.last() {
                                    Some(SortSpec { idx, direction }) if log_idx == *idx => {
                                        let dir = direction.clone();
                                        sort_by.pop_back();
                                        if dir == SortDirection::Ascending {
                                            sort_by.push_back(SortSpec::new(
                                                log_idx,
                                                SortDirection::Descending,
                                            ));
                                        }
                                    }
                                    _ => {
                                        if !extend {
                                            sort_by.clear();
                                        }
                                        sort_by
                                            .push_back(SortSpec::new(log_idx, SortDirection::Ascending));
                                    }
                                }

                                self.remap_rows =
                                    self.cell_delegate.remap(&data.data, &self.remap_spec_rows);
                                let tc = TableChange::Remap(RemapChanged(
                                    *axis.cross_axis(),
                                    self.remap_spec_rows.clone(),
                                    None,
                                ));
                                self.call_change_handlers(SubmitCommand::E(ctx), &tc);

                                ctx.request_paint();
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
                            self.selection
                                .move_focus(&TableAxis::Rows, VisOffset(1), self);
                        ctx.set_handled();
                    }
                    KbKey::ArrowUp => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Rows, VisOffset(-1), self);
                        ctx.set_handled();
                    }
                    KbKey::ArrowRight => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Columns, VisOffset(1), self);
                        ctx.set_handled();
                    }
                    KbKey::ArrowLeft => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Columns, VisOffset(-1), self);
                        ctx.set_handled();
                    }
                    KbKey::Character(s) if s == " " => {
                        // This is to match Excel
                        if ke.mods.ctrl() {
                            new_selection = self.selection.extend_in_axis(TableAxis::Columns, self);
                            ctx.set_handled();
                        } else if ke.mods.shift() {
                            new_selection = self.selection.extend_in_axis(TableAxis::Rows, self);
                            ctx.set_handled();
                        }

                        // TODO - when Ctrl + Shift, select full grid
                    }
                    _ => (),
                }
            }
            _ => match &mut self.editing {
                Editing::Cell { single_cell, child } => {
                    data.data.with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
                }
                _ => (),
            },
        }

        if let Some(sel) = new_selection {
            self.set_selection(ctx, sel);
            if !self.editing.is_active() {
                ctx.request_focus();
            }
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
            let rtc = self.config.resolve(env);
            // Todo: column moves / hiding etc
            self.column_measure.set_axis_properties(
                rtc.cell_border_thickness,
                self.cell_delegate.number_of_columns_in_data(&data.data),
                &Remap::Pristine,
            );

            self.remap_spec_rows = self.cell_delegate.initial_spec();
            self.remap_rows = self.cell_delegate.remap(&data.data, &self.remap_spec_rows);
            self.row_measure.set_axis_properties(
                rtc.cell_border_thickness,
                data.data.idx_len(),
                &self.remap_rows,
            );
            self.resolved_config = Some(rtc);
            self.call_change_handlers(
                SubmitCommand::L(ctx),
                &TableChange::Remap(RemapChanged(
                    TableAxis::Rows,
                    self.remap_spec_rows.clone(),
                    None,
                )),
            );
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

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &TableState<TableData>, data: &TableState<TableData>, _env: &Env) {
        if let Some(rtc) = &self.resolved_config {
            if !old_data.data.same(&data.data) {
                // Reapply sorting / filtering. May need to rate limit
                // and/or have some async way to notify back that sort is complete
                if !self.remap_spec_rows.is_empty() {
                    self.remap_rows = self.cell_delegate.remap(&data.data, &self.remap_spec_rows);
                }

                if old_data.data.idx_len() != data.data.idx_len() {
                    // need to deal with reordering and key columns etc

                    self.row_measure.set_axis_properties(
                        rtc.cell_border_thickness,
                        data.data.idx_len(),
                        &self.remap_rows,
                    );
                    ctx.request_layout(); // TODO: Work out if needed - if we were filling our area before
                }
                // Columns update from data
            }
        }
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
        bc.constrain(self.measured_size())
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

        self.paint_cells(ctx, &data.data, env, &cell_rect);
        self.paint_selections(ctx, &rtc, &cell_rect);

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

                ctx.with_save(|ctx|{
                    ctx.render_ctx.clip( Rect::from_origin_size(origin, size));
                    data.data.with(single_cell.log.row, |row| child.paint(ctx, row, env));
                });
            }
            _ => (),
        }
    }
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> CellDemap
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn get_log_idx(&self, axis: TableAxis, vis: &VisIdx) -> Option<LogIdx> {
        let remap = self.remap_for_axis(axis);
        remap.get_log_idx(*vis)
    }
}

impl <RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
Bindable for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
    where
        RowData: Data,
        TableData: IndexedData<Item = RowData, Idx = LogIdx>,
        ColDel: CellsDelegate<TableData>,
        ColumnMeasure: AxisMeasure,
        RowMeasure: AxisMeasure,
{

}