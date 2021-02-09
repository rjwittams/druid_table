use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{BoxConstraints, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx,
            PaintCtx, Point, Rect, Size, UpdateCtx, Widget, WidgetPod, InternalLifeCycle};

use crate::axis_measure::{AxisMeasure, AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::cells::Editing::Inactive;
use crate::columns::{CellCtx, CellRender};
use crate::config::{ResolvedTableConfig};
use crate::data::{IndexedData, Remapper};
use crate::render_ext::RenderContextExt;
use crate::selection::{CellRect, SingleCell, TableSelection};
use crate::table::TableState;
use crate::{EditorFactory, Remap, RemapSpec};
use druid_bindings::{bindable_self_body, BindableAccess};
use std::fmt::Debug;
use std::sync::Arc;
use std::ops::{Deref};
use std::collections::HashMap;
use std::time::Instant;
use priority_queue::PriorityQueue;
use std::cmp::Reverse;

pub trait CellsDelegate<TableData: IndexedData>:
    CellRender<TableData::Item> + Remapper<TableData> + EditorFactory<TableData::Item> + Debug
{
    fn data_columns(&self, data: &TableData) -> usize;
}

impl <TableData: IndexedData> CellsDelegate<TableData> for Arc<dyn CellsDelegate<TableData>> {
    fn data_columns(&self, data: &TableData) -> usize{
        self.as_ref().data_columns(data)
    }
}

impl <TableData: IndexedData>  CellRender<TableData::Item> for Arc<dyn CellsDelegate<TableData>> {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        //self.deref().init(ctx, env)
    }

    fn paint(&self, ctx: &mut PaintCtx, cell: &CellCtx, data: &TableData::Item, env: &Env) {
        self.deref().paint(ctx, cell, data, env)
    }

    fn event(&self, ctx: &mut EventCtx, cell: &CellCtx, event: &Event, data: &mut TableData::Item, env: &Env) {
        self.deref().event(ctx, cell, event, data, env)
    }

    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.deref().make_display(cell)
    }
}

impl <TableData: IndexedData> Remapper<TableData> for Arc<dyn CellsDelegate<TableData>> {
    fn sort_fixed(&self, idx: usize) -> bool {
        self.deref().sort_fixed(idx)
    }

    fn initial_spec(&self) -> RemapSpec {
        self.deref().initial_spec()
    }

    fn remap_items(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap {
        self.deref().remap_items(table_data, remap_spec)
    }
}

impl <TableData: IndexedData>  EditorFactory<TableData::Item> for Arc<dyn CellsDelegate<TableData>> {
    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.deref().make_editor(ctx)
    }
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

    fn handle_event<TableData: IndexedData<Item = RowData>>(
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

    fn start_editing<TableData: IndexedData<Item = RowData>>(
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

    fn stop_editing<TableData: IndexedData<Item = RowData>>(&mut self, _data: &mut TableData) {
        match self {
            Editing::Cell { .. } => {
                // Work out what to do with the previous pod if there is one.
                // We could have lazy editors (that don't write back to data immediately)
                // and w
                // Would need to give them data for their row
            }
            Editing::Inactive => {}
        }
        *self = Editing::Inactive
    }
}

pub struct Cells<TableData: IndexedData>
{
    start: Instant,
    cell_pods: HashMap<AxisPair<LogIdx>, Option<WidgetPod<TableData::Item, Box<dyn Widget<TableData::Item>>>>>,
    cells_lru: PriorityQueue<AxisPair<LogIdx>, Reverse<Instant>>,
    editing: Editing<TableData::Item>,
    dragging_selection: bool,
    phantom_td: PhantomData<TableData>,
}

impl <TableData: IndexedData> Cells<TableData>
{
    pub fn new() -> Cells<TableData> {
        Cells {
            start: Instant::now(),
            cell_pods: Default::default(),
            cells_lru: Default::default(),
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

    fn ensure_cell_pods(&mut self, data: &TableState<TableData>)->bool{
        let now = Instant::now();
        //log::info!("Ensuring pods {:?}", (Instant::now() - self.start).as_secs_f32() );

        let draw_rect = data.scroll_rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            self.measured_size(&data.measures),
        ));

        let cell_rect = CellRect::new(
            data.measures[TableAxis::Rows].vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            data.measures[TableAxis::Columns].vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );


        let mut added = 0;
        let mut visible = 0;
        let cell_delegate = &data.cells_del;
        for vis_row_idx in cell_rect.rows() {
            if let Some(log_row_idx) = data.remaps[TableAxis::Rows].get_log_idx(vis_row_idx) {
                for vis_col_idx in cell_rect.cols(){
                    if let Some(log_col_idx) = data.remaps[TableAxis::Columns].get_log_idx(vis_col_idx) {
                        visible += 1;
                        let sc = SingleCell::new(
                            AxisPair::new(vis_row_idx, vis_col_idx),
                            AxisPair::new(log_row_idx, log_col_idx),
                        );
                        let cell = CellCtx::Cell(&sc);

                        self.cell_pods.entry(sc.log.clone()).or_insert_with_key(|key| {
                            added += 1;
                            cell_delegate.make_display(&cell).map(WidgetPod::new)
                        });
                        self.cells_lru.push(sc.log.clone(), Reverse(now));
                    }
                }
            }
        }

        let max_widgets = (2.5 * (visible as f64)).floor() as isize;
        let removed = 0.max((self.cell_pods.len() as isize)- max_widgets);
        for _ in 0..removed {
            if let Some((log_cell, _)) = self.cells_lru.pop() {
                self.cell_pods.remove(&log_cell);
            }
        }

        if added > 0 {
            log::info!("Ensuring cells pix rect {:?}, cell rect {:?}, {}t/{}v/+{}/-{} widgets",
                       &data.scroll_rect, &cell_rect, self.cell_pods.len(), visible, added, removed);
        }

        added > 0 || removed > 0
    }

    fn paint_cells(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        env: &Env,
        rect: &CellRect,
    ) {
        let start = Instant::now();
        for vis_row_idx in rect.rows() {
            let col_remap = &data.remaps[TableAxis::Columns];
            let measures = &data.measures;

            if let Some(log_row_idx) = data.remaps[TableAxis::Rows].get_log_idx(vis_row_idx) {
                let table_data = &data.table_data;
                table_data.with(log_row_idx, |row| {
                    self.paint_row(
                        ctx,
                        env,
                        &data.resolved_config,
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

        //let duration = start.elapsed();
        //log::info!("Paint {:?} {} secs {:?}", (start - self.start).as_secs_f32(), duration.as_secs_f32(), rect)
    }

    fn paint_row(
        &mut self,
        ctx: &mut PaintCtx,
        env: &Env,
        rtc: &ResolvedTableConfig,
        cols: &mut impl Iterator<Item=VisIdx>,
        log_row_idx: LogIdx,
        vis_row_idx: VisIdx,
        row: &TableData::Item,
        col_remap: &Remap,
        measures: &AxisPair<AxisMeasure>,
    ) -> Option<()> {
        for vis_col_idx in cols {
            if let Some(log_col_idx) = col_remap.get_log_idx(vis_col_idx) {
                let sc = SingleCell::new(
                    AxisPair::new(vis_row_idx, vis_col_idx),
                    AxisPair::new(log_row_idx, log_col_idx),
                );

                let cell_pod = self.cell_pods.get_mut(&sc.log);

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
                    ctx.clip(padded_rect);
                    if let Some(Some(pod)) = cell_pod{
                        //log::info!("Paint cell {:?}, {:?}", &sc.vis, pod.paint_rect());
                        pod.paint(ctx, row, env)
                    }
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
                    data.table_data
                        .with(single_cell.log.row, |row| child.paint(ctx, row, env));
                });
            }
            _ => (),
        }
        Some(())
    }
}


impl<TableData: IndexedData> Widget<TableState<TableData>> for Cells<TableData>
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableState<TableData>,
        env: &Env,
    ) {
        let mut new_selection: Option<TableSelection> = None;

        match event {
            Event::MouseDown(me) => {
                if let Some(cell) = self.find_cell(data, &me.pos) {
                    if self.editing.is_editing(&cell) {
                        self.editing
                            .handle_event(ctx, event, &mut data.table_data, env);
                    } else {
                        if me.count == 1 {
                            let selected_cell = cell.clone();
                            if me.mods.meta() || me.mods.ctrl() {
                                new_selection = data.selection.add_selection(selected_cell.into());
                            } else if me.mods.shift() {
                                new_selection = data.selection.move_extent(selected_cell.into());
                            } else {
                                new_selection = Some(selected_cell.into());
                            }

                            ctx.set_handled();
                            self.editing.stop_editing(&mut data.table_data);
                            self.dragging_selection = true;
                            ctx.set_active(true);
                        } else if me.count == 2 {
                            let cd = &data.cells_del;
                            self.editing.start_editing(
                                ctx,
                                &mut data.table_data,
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
            Event::MouseUp(_) if self.dragging_selection => {
                self.dragging_selection = false;
                ctx.set_active(false);
            },
            Event::KeyDown(ke) if !self.editing.is_active() => {
                match &ke.key {
                    KbKey::ArrowDown => {
                        new_selection =
                            data.selection
                                .move_focus(TableAxis::Rows, VisOffset(1), &data.remaps);
                        ctx.set_handled();
                    }
                    KbKey::ArrowUp => {
                        new_selection =
                            data.selection
                                .move_focus(TableAxis::Rows, VisOffset(-1), &data.remaps);
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
                    data.table_data
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

        if let Some(foc) = data.selection.focus() {
            data.table_data.with_mut(foc.log.row, |item| {
                if let Some(Some(pod)) = self.cell_pods.get_mut(&foc.log) {
                    if pod.is_initialized() {
                        pod.event(
                            ctx,
                            event,
                            item,
                            env,
                        );
                    }
                }
            });
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<TableData>,
        env: &Env,
    ) {
        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                data.table_data.with(single_cell.log.row, |row| {
                    child.lifecycle(ctx, event, row, env)
                });
            }
            _ => (),
        }
        // TODO: visibility?
        for (log_cell, pod) in &mut self.cell_pods{
            if let Some(pod) = pod {
                data.table_data.with( log_cell.row, |row| {
                    if matches!(event, LifeCycle::Internal(InternalLifeCycle::RouteWidgetAdded)) || pod.is_initialized() {
                        pod.lifecycle(ctx, event, row, env);
                    }
                });
            }
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &TableState<TableData>,
        data: &TableState<TableData>,
        env: &Env,
    ) {
        if !old_data.table_data.same(&data.table_data)
            || !old_data.remaps.same(&data.remaps)
        {
            ctx.request_layout()
        }

        if !old_data.selection.same(&data.selection) {
            ctx.request_paint();
        }

        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                data.table_data.with(single_cell.log.row, |row| {
                    child.update(ctx, row, env)
                });
            }
            _ => (),
        }


        if !old_data.scroll_rect.same(&data.scroll_rect) {
            if self.ensure_cell_pods(data) {
                ctx.children_changed();
                ctx.request_anim_frame();
            }
        }

        // TODO: visible?
        // TODO: Stateless cell widgets?
        // TODO: Extract changed cells from data.table_data (extend IndexedData interface)
        for (log_cell, pod) in &mut self.cell_pods{
            if let Some(pod) = pod {
                if pod.is_initialized() {
                    data.table_data.with(log_cell.row, |row| {
                        pod.update(ctx, row, env);
                    });
                }
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
        //log::info!("Layout {:?}", (Instant::now() - self.start).as_secs_f32());

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
                    data.table_data.with(single_cell.log.row, |row| {
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

        let draw_rect = data.scroll_rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            measured,
        ));

        let cell_rect = CellRect::new(
            data.measures[TableAxis::Rows].vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            data.measures[TableAxis::Columns].vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );

        let row_measure = &data.measures[TableAxis::Rows];
        let col_measure = &data.measures[TableAxis::Columns];

        for vis_row_idx in cell_rect.rows() {
            if let Some(log_row_idx) = data.remaps[TableAxis::Rows].get_log_idx(vis_row_idx) {

                if let (Some(y), Some(height)) = (row_measure.first_pixel_from_vis(vis_row_idx), row_measure.pixels_length_for_vis(vis_row_idx)) {
                    data.table_data.with(log_row_idx, |row| {
                        for vis_col_idx in cell_rect.cols() {
                            if let Some(log_col_idx) = data.remaps[TableAxis::Columns].get_log_idx(vis_col_idx) {
                                if let (Some(x), Some(width)) = (col_measure.first_pixel_from_vis(vis_col_idx), col_measure.pixels_length_for_vis(vis_col_idx)) {
                                    if let Some(Some(cell_pod)) = self.cell_pods.get_mut(&AxisPair::new(log_row_idx, log_col_idx)) {
                                        if cell_pod.is_initialized()  {

                                            let size = cell_pod.layout(ctx, &BoxConstraints::tight(Size::new(width, height)), row, env);
                                            //log::info!("Cell {:?} size {}", (vis_row_idx, vis_col_idx), size);
                                            // TODO: could align the given size to different edge/corner
                                            cell_pod.set_origin(ctx, row, env, (x, y).into());
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableState<TableData>, env: &Env) {
        let rtc = &data.resolved_config;
        let rect = ctx.region().bounding_box();

        let draw_rect = rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            self.measured_size(&data.measures),
        ));

        let cell_rect = CellRect::new(
            data.measures[TableAxis::Rows].vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            data.measures[TableAxis::Columns].vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );

        ctx.fill(draw_rect, &rtc.cells_background);
        self.paint_cells(ctx, data, env, &cell_rect);
        self.paint_selections(ctx, data, &rtc, &cell_rect);

        self.paint_editing(ctx, data, env);
    }
}

impl<TableData> BindableAccess for Cells<TableData>
where
    TableData: IndexedData,
    TableData::Item: Data
{
    bindable_self_body!();
}
