use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, InternalLifeCycle, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx,
    WidgetPod
};

use crate::axis_measure::{AxisMeasure, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::columns::{CellCtx, DisplayFactory, HeaderInfo};
use crate::data::SortSpec;
use crate::ensured_pool::EnsuredPool;
use crate::numbers_table::LogIdxTable;
use crate::render_ext::RenderContextExt;
use crate::table::{TableState, PixelRange};
use crate::{IndexedData, Remap, SortDirection};
use druid::kurbo::PathEl;
use druid_bindings::{bindable_self_body, BindableAccess};
use std::collections::HashMap;

pub trait HeadersFromData {
    type TableData: IndexedData;
    type Header: Data;
    type Headers: IndexedData<Item = Self::Header>;
    fn get_headers(&self, table_data: &Self::TableData) -> Self::Headers;
}

pub struct SuppliedHeaders<Headers, TableData> {
    headers: Headers,
    phantom_td: PhantomData<TableData>,
}

impl<Headers, TableData> SuppliedHeaders<Headers, TableData> {
    pub fn new(headers: Headers) -> Self {
        SuppliedHeaders {
            headers,
            phantom_td: Default::default(),
        }
    }
}

impl<Headers: IndexedData + Clone, TableData: IndexedData> HeadersFromData
    for SuppliedHeaders<Headers, TableData>
where
    Headers::Item: Data,
{
    type TableData = TableData;
    type Header = Headers::Item;
    type Headers = Headers;
    fn get_headers(&self, _table_data: &Self::TableData) -> Headers {
        self.headers.clone()
    }
}

#[derive(Clone)]
pub struct HeadersFromIndices<TableData> {
    phantom_td: PhantomData<TableData>,
}

impl<TableData: Data> Data for HeadersFromIndices<TableData> {
    fn same(&self, _other: &Self) -> bool {
        true
    }
}

impl<TableData> Default for HeadersFromIndices<TableData> {
    fn default() -> Self {
        HeadersFromIndices::new()
    }
}

impl<TableData> HeadersFromIndices<TableData> {
    pub fn new() -> Self {
        HeadersFromIndices {
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedData> HeadersFromData for HeadersFromIndices<TableData> {
    type TableData = TableData;
    type Header = LogIdx;
    type Headers = LogIdxTable;

    fn get_headers(&self, table_data: &TableData) -> LogIdxTable {
        LogIdxTable::new(table_data.data_len())
    }
}

struct HeaderMoving {
    idx: VisIdx,
    first_px: f64,
    init_pos: f64,
    current_pos: f64,
}

impl HeaderMoving {
    pub fn new(idx: VisIdx, first_px: f64, init_pos: f64) -> Self {
        HeaderMoving {
            idx,
            first_px,
            init_pos,
            current_pos: init_pos,
        }
    }

    pub fn current_first_px(&self)->f64{
        self.first_px + self.offset()
    }

    pub fn offset(&self)->f64{
        self.current_pos - self.init_pos
    }
}

pub struct Headings<HeadersSource>
where
    HeadersSource: HeadersFromData
{
    axis: TableAxis,
    headers_source: HeadersSource,
    headers: Option<HeadersSource::Headers>,
    header_render: Box<dyn DisplayFactory<HeadersSource::Header>>,
    pods: EnsuredPool<
        LogIdx,
        Option<WidgetPod<HeadersSource::Header, Box<dyn Widget<HeadersSource::Header>>>>,
    >,
    allow_moves: bool,
    // TODO: combine these three (and) into a state machine enum as only one can be happening
    moving: Option<HeaderMoving>,
    resize_dragging: Option<VisIdx>,
    selection_dragging: bool,
}

impl<HeadersSource: HeadersFromData> Headings<HeadersSource> {
    pub fn new(
        axis: TableAxis,
        headers_source: HeadersSource,
        header_render: Box<dyn DisplayFactory<HeadersSource::Header>>,
        allow_moves: bool,
    ) -> Headings<HeadersSource> {
        Headings {
            axis,
            headers_source,
            headers: None,
            header_render,
            pods: Default::default(),
            allow_moves,
            moving: None,
            resize_dragging: None,
            selection_dragging: false,
        }
    }

    fn set_pix_length_for_axis(
        &mut self,
        measure: &mut AxisMeasure,
        remap: &Remap,
        ctx: &mut EventCtx,
        vis_idx: VisIdx,
        pixel: f64,
    ) {
        measure.set_far_pixel_for_vis(vis_idx, pixel, remap);
        ctx.request_layout();
    }

    fn ensure_pods(&mut self, data: &TableState<HeadersSource::TableData>) -> bool {
        let axis = self.axis;
        let cross_rem = &data.remap_specs[self.axis.cross_axis()];
        let sort_dirs: HashMap<LogIdx, SortSpec> = cross_rem
            .sort_by
            .iter()
            .enumerate()
            .map(|(ord, x)| (LogIdx(x.idx), SortSpec::new(ord, x.direction)))
            .collect();

        let header_render = &self.header_render;

        self.pods.ensure(
            data.log_idx_visible_for_axis(axis),
            |li| li,
            |log_main_idx| {
                let sort_spec = sort_dirs.get(&log_main_idx);
                let cell = CellCtx::Header(HeaderInfo::new(axis, log_main_idx, sort_spec));
                header_render.make_display(&cell).map(WidgetPod::new)
            },
        )
    }

}

fn draw_sort_indicator(ctx: &mut PaintCtx, sort_spec: &SortSpec, orig_rect: Rect) -> Rect {
    let rect = orig_rect.inset(-3.);
    let rad = rect.height() * 0.35;
    let up = sort_spec.direction == SortDirection::Ascending;

    let arrow = make_arrow(
        &Point::new(rect.max_x() - rad, rect.min_y()),
        up,
        rect.height(),
        rad,
    );
    ctx.render_ctx.stroke(&arrow[..], &Color::WHITE, 1.5);

    orig_rect.with_size((orig_rect.width() - (rad + 3.) * 2., orig_rect.height()))
}

fn make_arrow(top_point: &Point, up: bool, height: f64, head_rad: f64) -> [PathEl; 5] {
    let start_y = top_point.y;
    let tip_y = start_y + height;

    let (start_y, tip_y, mult) = if up {
        (tip_y, start_y, -1.)
    } else {
        (start_y, tip_y, 1.0)
    };
    let head_start_y = tip_y - (head_rad * mult);

    let mid_x = top_point.x;

    [
        PathEl::MoveTo((mid_x, start_y).into()),
        PathEl::LineTo((mid_x, tip_y).into()),
        PathEl::LineTo((mid_x - head_rad, head_start_y).into()),
        PathEl::MoveTo((mid_x, tip_y).into()),
        PathEl::LineTo((mid_x + head_rad, head_start_y).into()),
    ]
}

impl<HeadersSource> Widget<TableState<HeadersSource::TableData>>
    for Headings<HeadersSource>
where
    HeadersSource: HeadersFromData
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        let measure = &mut data.measures[self.axis];
        let remap = &data.remaps[self.axis];
        match event {
            Event::MouseDown(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if me.count == 2 {
                    let extend = me.mods.ctrl() || me.mods.meta();
                    if let Some(vis_idx) = measure.vis_idx_from_pixel(pix_main) {
                        if let Some(log_idx) = data.remaps[self.axis].get_log_idx(vis_idx) {
                            data.remap_specs[self.axis.cross_axis()].toggle_sort(log_idx, extend);
                        }
                        ctx.set_handled()
                    }
                } else if me.count == 1 {
                    //TODO: Combine lookups?
                    if let Some(idx) = measure.pixel_near_border(pix_main) {
                        if idx > VisIdx(0) && measure.can_resize(idx - VisOffset(1)) {
                            self.resize_dragging = Some(idx - VisOffset(1));
                            ctx.set_active(true);
                            ctx.set_cursor(self.axis.resize_cursor());
                            ctx.set_handled()
                        }
                    } else if let Some(idx) = measure.vis_idx_from_pixel(pix_main) {
                        let sel = &mut data.selection;
                        // Already selected so move headings:
                        if sel.fully_selects_heading(self.axis, idx) && self.allow_moves {
                            if let Some(first_px) = measure.first_pixel_from_vis(idx) {
                                self.moving = Some(HeaderMoving::new(
                                    idx,
                                    first_px,
                                    self.axis.main_pixel_from_point(&me.pos),
                                ));
                                ctx.set_active(true);
                            }
                        } else {
                            // Change the selection
                            if me.mods.shift() {
                                sel.extend_in_axis(self.axis, idx, &data.remaps);
                            } else {
                                sel.select_in_axis(self.axis, idx, &data.remaps);
                            }
                            self.selection_dragging = true;
                            ctx.set_active(true);
                        }
                        ctx.set_handled()
                    }
                }
            }
            Event::MouseMove(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                let over_idx = measure.vis_idx_from_pixel(pix_main);

                if let Some(resizing_idx) = self.resize_dragging {
                    self.set_pix_length_for_axis(measure, remap, ctx, resizing_idx, pix_main);

                    if me.buttons.is_empty() {
                        self.resize_dragging = None;
                    } else {
                        ctx.set_cursor(self.axis.resize_cursor());
                    }
                    ctx.set_handled()
                } else if let Some(moving) = &mut self.moving {
                    moving.current_pos = self.axis.main_pixel_from_point(&me.pos);

                    if let Some(log_idx) = remap.get_log_idx(moving.idx) {

                        data.overrides.measure[self.axis].entry(log_idx)
                            .or_insert_with(|| measure.pix_range_from_vis(moving.idx)
                                .unwrap_or_else(||PixelRange::new(moving.current_first_px(),
                                                                  measure.far_pixel_from_vis(moving.idx).unwrap_or(moving.current_pos + 100.) ))
                            ).move_to(moving.current_first_px());
                        ctx.request_layout();
                    }

                    ctx.request_paint();
                    ctx.set_handled();
                } else if self.selection_dragging {
                    if let Some(idx) = over_idx {
                        data.selection.extend_in_axis(self.axis, idx, &data.remaps);
                    }
                } else if let Some(resize_idx) = measure.pixel_near_border(pix_main) {
                    if resize_idx > VisIdx(0) {
                        let cursor = if measure.can_resize(resize_idx - VisOffset(1)) {
                            self.axis.resize_cursor()
                        } else {
                            &Cursor::NotAllowed
                        };
                        ctx.set_cursor(cursor);
                        ctx.set_handled();
                    }
                } else {
                    match over_idx {
                        Some(moving_idx)
                            if data.selection.fully_selects_heading(self.axis, moving_idx) && self.allow_moves =>
                        {
                            ctx.set_cursor(&Cursor::OpenHand)
                        }
                        _ => ctx.clear_cursor(),
                    }
                }
            }
            Event::MouseUp(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.resize_dragging {
                    self.set_pix_length_for_axis(measure, remap, ctx, idx, pix_main);
                    self.resize_dragging = None;
                    ctx.set_active(false);
                    ctx.set_handled();
                } else if let Some(moving) = self.moving.take() {
                    if let Some(log_idx) = remap.get_log_idx(moving.idx) {
                        data.overrides.measure[self.axis].remove(&log_idx);
                        ctx.request_layout();
                    }

                    if let Some(moved_to_idx) = measure.vis_idx_from_pixel(pix_main) {
                        data.explicit_header_move(self.axis, moving.idx, moved_to_idx)
                    }

                    ctx.request_paint();
                    ctx.set_active(false);
                    ctx.set_handled()
                } else if self.selection_dragging {
                    self.selection_dragging = false;
                    ctx.set_active(false);
                    ctx.set_handled()
                }
                ctx.clear_cursor()
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            self.headers = Some(self.headers_source.get_headers(&data.table_data));
            if self.ensure_pods(data) {
                ctx.children_changed();
                ctx.request_anim_frame();
            }
        }

        if let Some(headers) = &self.headers {
            for (log_idx, pod) in &mut self.pods.entries_mut() {
                if let Some(pod) = pod {
                    headers.with(*log_idx, |header| {
                        if matches!(
                            event,
                            LifeCycle::WidgetAdded
                                | LifeCycle::Internal(InternalLifeCycle::RouteWidgetAdded)
                        ) || pod.is_initialized()
                        {
                            pod.lifecycle(ctx, event, header, env);
                        }
                    });
                }
            }
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &TableState<HeadersSource::TableData>,
        data: &TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        if !old_data.same(data) {
            self.headers = Some(self.headers_source.get_headers(&data.table_data));
            if self.ensure_pods(data) {
                ctx.children_changed();
                ctx.request_anim_frame();
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let axis = self.axis;
        let rc = &data.resolved_config;
        let cross_axis_length = match axis {
            TableAxis::Columns => rc.col_header_height,
            TableAxis::Rows => rc.row_header_width,
        };
        let measure = &data.measures[axis];
        let size = bc.constrain(axis.size(measure.total_pixel_length(), cross_axis_length));
        let pods = &mut self.pods;
        let overrides = &data.overrides.measure[axis];
        if let Some(headers) = &self.headers {
            for vis_idx in data.vis_idx_visible_for_axis(axis) {
                if let (Some(found_px), Some(log_idx)) = (
                    measure.pix_range_from_vis(vis_idx),
                    data.remaps[axis].get_log_idx(vis_idx),
                ) {
                    let px = overrides.get(&log_idx).unwrap_or(&found_px);

                    if let Some(Some(pod)) = pods.get_mut(&log_idx) {
                        headers.with(log_idx, |header| {
                            if pod.is_initialized() {
                                let cell_size = axis.size(px.extent(), cross_axis_length);
                                pod.layout(
                                    ctx,
                                    &BoxConstraints::tight(cell_size).loosen(),
                                    header,
                                    env,
                                );
                                let origin = axis.coords(px.p_0, 0.).into();
                                pod.set_origin(ctx, header, env, origin);
                            }
                        });
                    }
                }
            }
        }
        size
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        let axis = self.axis;
        let measure = &data.measures[axis];
        let overrides = &data.overrides.measure[axis];
        let remap = &data.remaps[axis];
        let rtc = &data.resolved_config;

        let indices_selection = data.selection.to_axis_selection(axis, &data.remaps);
        let rect = ctx.region().bounding_box();

        // TODO build on change of spec
        let cross_rem = &data.remap_specs[self.axis.cross_axis()];
        let sort_dirs: HashMap<LogIdx, SortSpec> = cross_rem
            .sort_by
            .iter()
            .enumerate()
            .map(|(ord, x)| (LogIdx(x.idx), SortSpec::new(ord, x.direction)))
            .collect();

        ctx.fill(rect, &rtc.header_background);

        let (p0, p1) = self.axis.pixels_from_rect(&rect);
        let (start_main, end_main) = measure.vis_range_from_pixels(p0, p1);

        let pods = &mut self.pods;
        if let Some(headers) = &self.headers {
            for vis_idx in VisIdx::range_inc_iter(start_main, end_main) {
                if let (Some(found_px), Some(log_idx)) = (
                    measure.pix_range_from_vis(vis_idx),
                    data.remaps[axis].get_log_idx(vis_idx),
                ) {
                    let px = overrides.get(&log_idx).unwrap_or(&found_px);

                    let cell_rect = Rect::from_origin_size(
                        axis.cell_origin(px.p_0, 0.),
                        axis.size(px.extent(), rtc.cross_axis_length(&axis)),
                    );

                    if indices_selection.vis_index_selected(vis_idx) {
                        ctx.fill(cell_rect, &rtc.header_selected_background);
                    }

                    let padded_rect = cell_rect.inset(-rtc.cell_padding);
                    if let Some(log_idx) = remap.get_log_idx(vis_idx) {
                        let sort_spec = sort_dirs.get(&log_idx);

                        headers.with(log_idx, |col_name| {
                            ctx.with_save(|ctx| {
                                let clip_rect = if let Some(sort_spec) = sort_spec {
                                    draw_sort_indicator(ctx, sort_spec, padded_rect)
                                } else {
                                    padded_rect
                                };
                                ctx.clip(clip_rect);

                                if let Some(Some(pod)) = pods.get_mut(&log_idx) {
                                    pod.paint(ctx, col_name, env);
                                }
                            });
                        });

                        ctx.stroke_bottom_left_border(
                            &cell_rect,
                            &rtc.cells_border,
                            rtc.cell_border_thickness,
                        );
                    }
                }
            }
        }

        // if let Some(moving) = &self.moving {
        //     if let (Some(first_pix), Some(pixels_length)) = (
        //         measure.first_pixel_from_vis(moving.idx),
        //         measure.pixels_length_for_vis(moving.idx),
        //     ) {
        //         let offset = moving.current_pos - moving.init_pos;
        //
        //         let header_rect = Rect::from_origin_size(
        //             axis.cell_origin(first_pix, 0.) + self.axis.coords(offset, 0.),
        //             axis.size(pixels_length, rtc.cross_axis_length(&axis)),
        //         );
        //
        //         ctx.render_ctx.stroke(header_rect, &Color::TEAL, 1.5);
        //     }
        // }
    }
}

impl<HeadersSource> BindableAccess for Headings<HeadersSource>
where
    HeadersSource: HeadersFromData
{
    bindable_self_body!();
}
