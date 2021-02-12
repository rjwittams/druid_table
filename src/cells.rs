use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    BoxConstraints, Data, Env, Event, EventCtx, InternalLifeCycle, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx, Widget, WidgetPod,
};

use crate::axis_measure::{AxisMeasure, AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::cells::Editing::Inactive;
use crate::columns::{CellCtx, DisplayFactory};
use crate::config::ResolvedTableConfig;
use crate::data::{IndexedData, Remapper};
use crate::ensured_pool::EnsuredPool;
use crate::render_ext::RenderContextExt;
use crate::selection::{CellDemap, CellRect, SingleCell, TableSelection};
use crate::table::TableState;
use crate::{Remap, RemapSpec};
use druid_bindings::{bindable_self_body, BindableAccess};
use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::collections::hash_map::{Iter, IterMut};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;
use druid::widget::Axis;

pub trait CellsDelegate<TableData: IndexedData>:
    DisplayFactory<TableData::Item> + Remapper<TableData> + Debug
{
    fn data_columns(&self, data: &TableData) -> usize;
}

impl<TableData: IndexedData> CellsDelegate<TableData> for Arc<dyn CellsDelegate<TableData>> {
    fn data_columns(&self, data: &TableData) -> usize {
        self.as_ref().data_columns(data)
    }
}

impl<TableData: IndexedData> DisplayFactory<TableData::Item> for Arc<dyn CellsDelegate<TableData>> {
    fn make_display(
        &self,
        cell: &CellCtx,
    ) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.deref().make_display(cell)
    }

    fn make_editor(
        &self,
        ctx: &CellCtx,
    ) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.deref().make_editor(ctx)
    }
}

impl<TableData: IndexedData> Remapper<TableData> for Arc<dyn CellsDelegate<TableData>> {
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
        if let Editing::Cell { single_cell, child } = self {
            data.with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
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

pub struct Cells<TableData: IndexedData> {
    start: Instant,
    cell_pool: EnsuredPool<
        AxisPair<LogIdx>,
        Option<WidgetPod<TableData::Item, Box<dyn Widget<TableData::Item>>>>,
    >,
    editing: Editing<TableData::Item>,
    dragging_selection: bool,
    phantom_td: PhantomData<TableData>,
}

impl<TableData: IndexedData> Cells<TableData> {
    pub fn new() -> Cells<TableData> {
        Cells {
            start: Instant::now(),
            cell_pool: Default::default(),
            editing: Inactive,
            dragging_selection: false,
            phantom_td: PhantomData::default(),
        }
    }

    fn ensure_cell_pods(&mut self, data: &TableState<TableData>) -> bool {
        let draw_rect = data.visible_rect();
        let cell_rect = data.measures.cell_rect_from_pixels(draw_rect);
        let cell_delegate = &data.cells_del;

        let single_cells = cell_rect.cells().flat_map(|vis| {
            data.remaps
                .get_log_cell(&vis)
                .map(|log| SingleCell::new(vis, log))
        });

        let changed = self.cell_pool.ensure(
            single_cells,
            |sc| &sc.log,
            |sc| {
                let cell = CellCtx::Cell(&sc);
                cell_delegate.make_display(&cell).map(WidgetPod::new)
            },
        );

        changed
    }

    fn paint_cells(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<TableData>,
        env: &Env,
        rect: &CellRect,
    ) {
        let measures = &data.measures;
        let table_data = &data.table_data;
        let rtc = &data.resolved_config;
        for vis in rect.cells() {
            if let Some(log) = data.remaps.get_log_cell(&vis) {
                table_data.with(log.row, |row| {
                    if let Some(cell_rect) = measures.pixel_rect_for_cell(vis) {
                        let padded_rect = cell_rect.inset(-rtc.cell_padding);

                        ctx.with_save(|ctx| {
                            ctx.clip(padded_rect);
                            if let Some(Some(pod)) = self.cell_pool.get_mut(&log) {
                                pod.paint(ctx, row, env)
                            }
                        });

                        ctx.stroke_bottom_left_border(
                            &cell_rect,
                            &rtc.cells_border,
                            rtc.cell_border_thickness,
                        );
                    }
                });
            }
        }
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
            CellRect::point(focus).to_pixel_rect(&data.measures)?,
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
                // TODO: excessive unwrapping
                let rect = CellRect::point(single_cell.vis).to_pixel_rect(&data.measures)?;

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

impl<TableData: IndexedData> Widget<TableState<TableData>> for Cells<TableData> {
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
                if let Some(cell) = data.find_cell(me.pos) {
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

                            //ctx.set_handled();
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
                if let Some(cell) = data.find_cell( me.pos) {
                    new_selection = data.selection.move_extent(cell.into());
                }
            }
            Event::MouseUp(_) if self.dragging_selection => {
                self.dragging_selection = false;
                ctx.set_active(false);
            }
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
            _ => (),
        }

        if let Some(sel) = new_selection {
            data.selection = sel;
            if data.selection.has_focus() && !self.editing.is_active() {
                 ctx.request_focus();
            }
        }

        if let Editing::Cell { single_cell, child } = &mut self.editing {
            if child.is_initialized() {
                data.table_data
                    .with_mut(single_cell.log.row, |row| child.event(ctx, event, row, env));
            }
        }

        if let Some(foc) = data.selection.focus() {
            let mut delivered = false;

            data.table_data.with_mut(foc.log.row, |item| {
                if let Some(Some(pod)) = self.cell_pool.get_mut(&foc.log) {
                    if pod.is_initialized() {
                        pod.event(ctx, event, item, env);
                        delivered = true;
                    }
                }
            });
            //log::info!("Wanted to forward event to focused cell {:?} {:?}", event, delivered);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<TableData>,
        env: &Env,
    ) {
        if let Editing::Cell { single_cell, child } = &mut self.editing {
            data.table_data.with(single_cell.log.row, |row| {
                child.lifecycle(ctx, event, row, env)
            });
        }
        // TODO: visibility?
        for (log_cell, pod) in &mut self.cell_pool.entries_mut() {
            if let Some(pod) = pod {
                data.table_data.with(log_cell.row, |row| {
                    if matches!(
                        event,
                        LifeCycle::WidgetAdded
                            | LifeCycle::Internal(InternalLifeCycle::RouteWidgetAdded)
                    ) || pod.is_initialized()
                    {
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
        if !old_data.table_data.same(&data.table_data) || !old_data.remaps.same(&data.remaps) {
            ctx.request_layout()
        }

        if !old_data.selection.same(&data.selection) {
            ctx.request_paint();
        }

        match &mut self.editing {
            Editing::Cell { single_cell, child } => {
                data.table_data
                    .with(single_cell.log.row, |row| child.update(ctx, row, env));
            }
            _ => (),
        }

        if !old_data.scroll_rect.same(&data.scroll_rect) {
            if self.ensure_cell_pods(data) {
                ctx.children_changed();
                ctx.request_anim_frame();
            }
        }

        // TODO: Stateless cell widgets?
        // TODO: Extract changed cells from data.table_data (extend IndexedData interface)
        for (log_cell, pod) in &mut self.cell_pool.entries_mut() {
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

        if let Editing::Cell { single_cell, child } = &mut self.editing {
            let vis = &single_cell.vis;

            let pixels_len = data.measures.zip_with(&vis, |m, v| m.pixels_length_for_vis(*v)).opt();
            let first_pix = data.measures.zip_with(&vis, |m, v| m.first_pixel_from_vis(*v)).opt();

            if let (Some(size), Some(origin)) = ( pixels_len.as_ref().map(AxisPair::size), first_pix.as_ref().map(AxisPair::point) ) {
                let bc = BoxConstraints::tight(size).loosen();
                data.table_data.with(single_cell.log.row, |row| {
                    child.layout(ctx, &bc, row, env);
                    child.set_origin(ctx, row, env, origin)
                });
            }
        }
        let measured = data.measures.measured_size();
        let size = bc.constrain(measured);

        let draw_rect = data
            .scroll_rect
            .intersect(Rect::from_origin_size(Point::ZERO, measured));

        let cell_rect = data.measures.cell_rect_from_pixels(draw_rect) ;

        for vis in cell_rect.cells(){
            if let Some(log) = data.remaps.get_log_cell(&vis){
                data.table_data.with(log.row, |row| {
                    if let Some(Some(cell_pod)) = self
                        .cell_pool
                        .get_mut(&log)
                    {
                        if let Some(vis_rect) = CellRect::point(vis).to_pixel_rect(&data.measures) {
                            if cell_pod.is_initialized() {
                                cell_pod.layout(
                                    ctx,
                                    &BoxConstraints::tight(vis_rect.size()).loosen(),
                                    row,
                                    env,
                                );
                                // TODO: could align the given size to different edge/corner
                                cell_pod.set_origin(ctx, row, env, vis_rect.origin());
                            }
                        }
                    }
                });
            }
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableState<TableData>, env: &Env) {
        let rtc = &data.resolved_config;
        let rect = ctx.region().bounding_box();

        let draw_rect = rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            data.measures.measured_size(),
        ));

        let cell_rect = data.measures.cell_rect_from_pixels(draw_rect);

        ctx.fill(draw_rect, &rtc.cells_background);
        self.paint_cells(ctx, data, env, &cell_rect);
        self.paint_selections(ctx, data, &rtc, &cell_rect);

        self.paint_editing(ctx, data, env);
    }
}

impl<TableData> BindableAccess for Cells<TableData>
where
    TableData: IndexedData,
    TableData::Item: Data,
{
    bindable_self_body!();
}
