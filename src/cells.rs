use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, Selector, Size, UpdateCtx, Widget, WidgetPod,
};

use crate::axis_measure::{AxisMeasure, AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::cells::Editing::Inactive;
use crate::columns::{CellCtx, CellRender};
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{IndexedData, Remapper};
use crate::render_ext::RenderContextExt;
use crate::selection::{CellRect, SingleCell, TableSelection};
use crate::table::TableState;
use crate::{EditorFactory, IndexedItems, Remap};
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

    fn is_editing(&self, cell: &SingleCell) -> bool {
        match self {
            Editing::Cell { single_cell, .. } => single_cell.vis.eq(&cell.vis),
            _ => false,
        }
    }

    fn handle_event<TableData: IndexedItems<Idx = LogIdx, Item = RowData>>(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableData,
        env: &Env,
    ) {
        match self {
            Editing::Cell {
                ref single_cell,
                ref mut child,
            } => {
                data.with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
            }
            _ => {}
        }
    }

    fn start_editing<TableData: IndexedItems<Item = RowData>>(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut TableData,
        cell: &SingleCell,
        make_editor: impl FnMut(&CellCtx) -> Option<Box<dyn Widget<RowData>>>,
    ) {
        self.stop_editing(data);
        let mut me = make_editor;
        let cell_ctx = CellCtx::Cell(&cell);
        if let Some(editor) = me(&cell_ctx) {
            let pod = WidgetPod::new(editor);

            *self = Editing::Cell {
                single_cell: cell.clone(),
                child: pod,
            };

            ctx.children_changed();
            ctx.request_layout();
            ctx.set_handled();
        }
    }

    fn stop_editing<TableData: IndexedItems<Item = RowData>>(&mut self, data: &mut TableData) {
        match self {
            Editing::Cell { single_cell, child } => {
                // Work out what to do with the previous pod if there is one.
                // We could have lazy editors (that don't write back to data immediately) and send them a special command saying 'you are being shut down'.
                // Would need to give them data for their row
            }
            Editing::Inactive => {}
        }
        *self = Editing::Inactive
    }
}

pub struct Cells<TableData, CellDel>
where
    TableData: IndexedData<Idx = LogIdx>,
    TableData::Item: Data,
    CellDel: CellsDelegate<TableData>, // The length is the number of columns
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    cell_delegate: CellDel,
    editing: Editing<TableData::Item>,
    dragging_selection: bool,
    phantom_td: PhantomData<TableData>,
}

impl<TableData, CellDel> Cells<TableData, CellDel>
where
    TableData: IndexedData<Idx = LogIdx>,
    TableData::Item: Data,
    CellDel: CellsDelegate<TableData>,
{
    pub fn new(config: TableConfig, cells_delegate: CellDel) -> Cells<TableData, CellDel> {
        Cells {
            config,
            resolved_config: None,
            cell_delegate: cells_delegate,
            editing: Inactive,
            dragging_selection: false,
            phantom_td: PhantomData::default(),
        }
    }

    fn find_cell(&self, data: &TableState<TableData>, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            data.measures[TableAxis::Rows].vis_idx_from_pixel(pos.y)?,
            data.measures[TableAxis::Columns].vis_idx_from_pixel(pos.x)?,
        );
        let log_row = data.remaps[TableAxis::Rows].get_log_idx(r)?;
        let log_col = data.remaps[TableAxis::Columns].get_log_idx(c)?;
        Some(SingleCell::new(
            AxisPair::new(r, c),
            AxisPair::new(log_row, log_col),
        ))
    }

    fn measured_size(&mut self, measures: &AxisPair<AxisMeasure>) -> Size {
        measures.map(|m| m.total_pixel_length()).size()
    }

    fn paint_cells(
        &self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        env: &Env,
        rect: &CellRect,
    ) {
        for vis_row_idx in rect.rows() {
            let col_remap = &data.remaps[TableAxis::Columns];
            let measures = &data.measures;

            if let Some(log_row_idx) = data.remaps[TableAxis::Rows].get_log_idx(vis_row_idx) {
                let table_data = &data.data;
                table_data.with(log_row_idx, |row| {
                    self.paint_row(
                        ctx,
                        env,
                        &mut rect.cols(),
                        log_row_idx,
                        vis_row_idx,
                        row,
                        col_remap,
                        measures,
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
        row: &TableData::Item,
        col_remap: &Remap,
        measures: &AxisPair<AxisMeasure>,
    ) -> Option<()> {
        if let Some(rtc) = &self.resolved_config {
            for vis_col_idx in cols {
                if let Some(log_col_idx) = col_remap.get_log_idx(vis_col_idx) {
                    let sc = SingleCell::new(
                        AxisPair::new(vis_row_idx, vis_col_idx),
                        AxisPair::new(log_row_idx, log_col_idx),
                    );

                    let cell_rect = Rect::from_origin_size(
                        measures
                            .zip_with(&sc.vis, |m, vis| m.first_pixel_from_vis(*vis))
                            .opt()?
                            .point(),
                        measures
                            .zip_with(&sc.vis, |m, vis| m.pixels_length_for_vis(*vis))
                            .opt()?
                            .size(),
                    );
                    let padded_rect = cell_rect.inset(-rtc.cell_padding);

                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
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
        Some(())
    }

    fn paint_selections(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        rtc: &ResolvedTableConfig,
        cell_rect: &CellRect,
    ) -> Option<()> {
        let selected = data.selection.get_drawable_selections(cell_rect);

        let sel_color = &rtc.selection_color;
        let sel_fill = &sel_color.clone().with_alpha(0.2);

        for range_rect in &selected.ranges {
            if let Some(range_draw_rect) = range_rect.to_pixel_rect(&data.measures) {
                ctx.fill(range_draw_rect, sel_fill);
                ctx.stroke(range_draw_rect, sel_color, rtc.cell_border_thickness)
            }
        }

        let focus = selected.focus?;

        ctx.stroke(
            CellRect::point(focus.row, focus.col).to_pixel_rect(&data.measures)?,
            &rtc.focus_color,
            (rtc.cell_border_thickness * 1.5).min(2.),
        );
        Some(())
    }

    fn paint_editing(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        env: &Env,
    ) -> Option<()> {
        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                let vis = &single_cell.vis;
                // TODO: excessive unwrapping
                let rect = CellRect::point(vis.row, vis.col).to_pixel_rect(&data.measures)?;

                ctx.with_save(|ctx| {
                    ctx.render_ctx.clip(rect);
                    data.data
                        .with(single_cell.log.row, |row| child.paint(ctx, row, env));
                });
            }
            _ => (),
        }
        Some(())
    }
}

pub const INIT_CELLS: Selector<()> = Selector::new("druid-builtin.table.init-cells");
pub const SORT_CHANGED: Selector<TableAxis> = Selector::new("druid-builtin.table.sort-changed");

impl<TableData, ColDel> Widget<TableState<TableData>> for Cells<TableData, ColDel>
where
    TableData: IndexedData<Idx = LogIdx>,
    TableData::Item: Data,
    ColDel: CellsDelegate<TableData>,
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
                        } else {
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
                                self.editing.start_editing(
                                    ctx,
                                    &mut data.data,
                                    &cell,
                                    |cell_ctx| cd.make_editor(cell_ctx),
                                );
                            }
                        }
                    }
                }
                Event::MouseMove(me) if !self.editing.is_active() && self.dragging_selection => {
                    if let Some(cell) = self.find_cell(data, &me.pos) {
                        new_selection = data.selection.move_extent(cell.into());
                    }
                }
                Event::MouseUp(me) if self.dragging_selection => {
                    self.dragging_selection = false;
                    ctx.set_active(false);
                }
                Event::Command(cmd) => {
                    if let Some(_) = cmd.get(INIT_CELLS) {
                        data.remap_specs[TableAxis::Rows] = self.cell_delegate.initial_spec();
                        remap_changed[TableAxis::Rows] = true;
                        remap_changed[TableAxis::Columns] = true;
                    } else if let Some(ax) = cmd.get(SORT_CHANGED) {
                        remap_changed[*ax] = true;
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
                            new_selection = data.selection.move_focus(
                                TableAxis::Rows,
                                VisOffset(1),
                                &data.remaps,
                            );
                            ctx.set_handled();
                        }
                        KbKey::ArrowUp => {
                            new_selection = data.selection.move_focus(
                                TableAxis::Rows,
                                VisOffset(-1),
                                &data.remaps,
                            );
                            ctx.set_handled();
                        }
                        KbKey::ArrowRight => {
                            new_selection = data.selection.move_focus(
                                TableAxis::Columns,
                                VisOffset(1),
                                &data.remaps,
                            );
                            ctx.set_handled();
                        }
                        KbKey::ArrowLeft => {
                            new_selection = data.selection.move_focus(
                                TableAxis::Columns,
                                VisOffset(-1),
                                &data.remaps,
                            );
                            ctx.set_handled();
                        }
                        KbKey::Character(s) if s == " " => {
                            // This is to match Excel
                            if ke.mods.meta() || ke.mods.ctrl() {
                                new_selection = data
                                    .selection
                                    .extend_from_focus_in_axis(&TableAxis::Columns, &data.remaps);
                                ctx.set_handled();
                            } else if ke.mods.shift() {
                                new_selection = data
                                    .selection
                                    .extend_from_focus_in_axis(&TableAxis::Rows, &data.remaps);
                                ctx.set_handled();
                            }

                            // TODO - when Ctrl + Shift, select full grid
                        }
                        KbKey::Copy => log::info!("Copy"),
                        k => log::info!("Key {:?}", k),
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
            if remap_changed[TableAxis::Rows] {
                data.remap_axis(TableAxis::Rows, |d, s| self.cell_delegate.remap_items(d, s));
                data.measures[TableAxis::Rows].set_axis_properties(
                    rtc.cell_border_thickness,
                    data.data.idx_len(),
                    &data.remaps[TableAxis::Rows],
                );
                ctx.request_layout(); // Could avoid if we know we overflow scroll?
            }
            if remap_changed[TableAxis::Columns] {
                data.measures[TableAxis::Columns].set_axis_properties(
                    rtc.cell_border_thickness,
                    self.cell_delegate.number_of_columns_in_data(&data.data),
                    &data.remaps[TableAxis::Columns],
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
            ctx.submit_command(INIT_CELLS.with(()), ctx.widget_id());
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
        // TODO move all sorting up to table level so we don't need commands
        if !old_data.data.same(&data.data) || !old_data.remap_specs.same(&data.remap_specs) {
            ctx.submit_command(SORT_CHANGED.with(TableAxis::Rows), ctx.widget_id());
        }
        if !old_data.selection.same(&data.selection) {
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
                (|| -> Option<_> {
                    let bc = BoxConstraints::tight(
                        data.measures
                            .zip_with(&vis, |m, v| m.pixels_length_for_vis(*v))
                            .opt()?
                            .size(),
                    );
                    let origin = data
                        .measures
                        .zip_with(&vis, |m, v| m.first_pixel_from_vis(*v))
                        .opt()?
                        .point();
                    data.data.with(single_cell.log.row, |row| {
                        let size = child.layout(ctx, &bc, row, env);
                        child.set_layout_rect(ctx, row, env, Rect::from_origin_size(origin, size))
                    });
                    Some(())
                })();
            }
            _ => (),
        }
        let measured = self.measured_size(&data.measures);
        let size = bc.constrain(measured);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableState<TableData>, env: &Env) {
        self.cell_delegate.init(ctx, env); // TODO reduce calls? Invalidate on some changes

        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        let draw_rect = rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            self.measured_size(&data.measures),
        ));

        ctx.fill(draw_rect, &rtc.cells_background);

        let cell_rect = CellRect::new(
            data.measures[TableAxis::Rows].vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            data.measures[TableAxis::Columns].vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );

        self.paint_cells(ctx, data, env, &cell_rect);
        self.paint_selections(ctx, data, &rtc, &cell_rect);

        self.paint_editing(ctx, data, env);
    }
}

impl<TableData, CellsDel> Bindable for Cells<TableData, CellsDel>
where
    TableData: IndexedData<Idx = LogIdx>,
    TableData::Item: Data,
    CellsDel: CellsDelegate<TableData>,
{
}
