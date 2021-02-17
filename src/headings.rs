use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, InternalLifeCycle, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx, WidgetPod,
};

use crate::axis_measure::{AxisMeasure, LogIdx, TableAxis, VisIdx, VisOffset, PixelLengths, OverriddenPixelLengths};
use crate::columns::{CellCtx, DisplayFactory, HeaderInfo};
use crate::data::{SortSpec, PartialEqData};
use crate::ensured_pool::EnsuredPool;
use crate::numbers_table::LogIdxTable;
use crate::render_ext::RenderContextExt;
use crate::table::{PixelRange, TableState};
use crate::{IndexedData, Remap, SortDirection, AxisMeasurementType};
use druid::kurbo::PathEl;
use druid_bindings::{bindable_self_body, BindableAccess};
use std::collections::HashMap;
use im::Vector;
use itertools::Itertools;
use std::cmp::Ordering;

pub trait StaticHeader {
    fn header_levels()->usize;
    fn header_compare(level: LogIdx, a: &Self, b: &Self)->Ordering;
}

impl StaticHeader for String {
    fn header_levels() -> usize {
        1
    }

    fn header_compare(level: LogIdx, a: &Self, b: &Self) -> Ordering {
        a.cmp(b)
    }
}

// min const generics where are you
impl <T: Ord> StaticHeader for [T;2] {
    fn header_levels() -> usize {
        2
    }

    fn header_compare(level: LogIdx, a: &Self, b: &Self) -> Ordering {
        let level = level.0.clamp(0, 1);
        a[level].cmp(&b[level])
    }
}

pub trait Headers: IndexedData{
    fn header_levels(&self)->usize;
    fn header_compare(&self, level: LogIdx, a: &Self::Item, b: &Self::Item)->Ordering;
}

impl <H : StaticHeader + Data> Headers for Vector<H>{
    fn header_levels(&self) -> usize {
        H::header_levels()
    }

    fn header_compare(&self, level: LogIdx, a: &H, b: &H) -> Ordering {
        H::header_compare(level, a, b)
    }
}

pub trait HeadersFromData {
    type TableData: IndexedData;
    type Header: Data;
    type Headers: Headers<Item = Self::Header>;
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

impl<H: Headers, TableData: IndexedData> HeadersFromData
    for SuppliedHeaders<H, TableData>
{
    type TableData = TableData;
    type Header = H::Item;
    type Headers = H;
    fn get_headers(&self, _table_data: &Self::TableData) -> H {
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
        Self {
            phantom_td: PhantomData,
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

    pub fn current_first_px(&self) -> f64 {
        self.first_px + self.offset()
    }

    pub fn offset(&self) -> f64 {
        self.current_pos - self.init_pos
    }
}

struct Resolved<Headers>{
    headers: Headers,
    level_remap: Remap,
    level_measure: AxisMeasure,
    visible_headings: Vec<((LogIdx, LogIdx), usize)> // ((level, field), field_span)
}

#[derive(Copy, Clone, PartialEq, Data, Debug)]
enum HeaderAxis{
    Item,
    Level
}

#[derive(Copy, Clone, Data, Debug)]
struct ResizeDragging{
    header_axis: HeaderAxis,
    idx: VisIdx
}

impl ResizeDragging {
    pub fn new(header_axis: HeaderAxis, idx: VisIdx) -> Self {
        ResizeDragging { header_axis, idx }
    }
}


pub struct Headings<HeadersSource>
where
    HeadersSource: HeadersFromData,
{
    axis: TableAxis,
    headers_source: HeadersSource,
    resolved: Option<Resolved<HeadersSource::Headers>>,
    header_render: Box<dyn DisplayFactory<HeadersSource::Header>>,
    pods: EnsuredPool<
        (LogIdx, LogIdx), // Level, Field
        Option<WidgetPod<HeadersSource::Header, Box<dyn Widget<HeadersSource::Header>>>>,
    >,
    allow_moves: bool,
    // TODO: combine these three (and) into a state machine enum as only one can be happening
    moving: Option<HeaderMoving>,
    resize_dragging: Option<ResizeDragging>,
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
            resolved: None,
            header_render,
            pods: Default::default(),
            allow_moves,
            moving: None,
            resize_dragging: None,
            selection_dragging: false,
        }
    }



    fn ensure_pods(&mut self, data: &TableState<HeadersSource::TableData>) -> bool {
        if let Some(r) = &self.resolved {
            let axis = self.axis;
            let header_render = &self.header_render;

            self.pods.ensure(
                r.visible_headings.iter(),
                |pair| &pair.0,
                |((level, header_idx), span)| {
                    let cell = CellCtx::Header(HeaderInfo::new(axis, *level, *header_idx));
                    header_render.make_display(&cell).map(WidgetPod::new)
                },
            )
        }else{
            false
        }
    }

    fn refresh_headers(&mut self, data: &TableState<HeadersSource::TableData>) {
        let rc = &data.resolved_config;
        let headers = self.headers_source.get_headers(&data.table_data);
        let levels = headers.header_levels();
        let mut resolved = if let Some(mut r) = self.resolved.take() {
            r.headers = headers;
            // Use reversed as we want the header levels to grow backwards from the edge of the table
            r.level_remap = Remap::Reversed(levels);
            r
        } else {
            let cross_axis_length = match self.axis {
                TableAxis::Columns => rc.col_header_height,
                TableAxis::Rows => rc.row_header_width,
            };
            let level_measure = AxisMeasure::new(AxisMeasurementType::Individual, cross_axis_length);
            Resolved { headers, level_measure, level_remap: Remap::Reversed(levels), visible_headings: Vec::new() }
        };

        resolved.level_measure.set_axis_properties(rc.cell_border_thickness, levels, &resolved.level_remap);

        let levels = (0..levels).map(LogIdx);

        let headers = &resolved.headers;
        resolved.visible_headings = levels.flat_map(|log_level|{
            data.log_idx_in_visible_order_for_axis(self.axis)
                .peekable()
                .batching(move |it|{
                    it.next().and_then(|log_field_idx|{
                        headers.get_clone(log_field_idx).as_ref().map(|header_val| {
                            let count = it.peeking_take_while(|next_log_field_idx| {
                                headers.get_clone(*next_log_field_idx).as_ref().map(|next_header_val|{
                                    headers.header_compare(log_level, header_val, next_header_val) == Ordering::Equal
                                }).unwrap_or(false)
                            }).count();
                            ((log_level, log_field_idx), count)
                        })
                    })
                })
        }).collect();

        self.resolved = Some(resolved);
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

impl<HeadersSource> Widget<TableState<HeadersSource::TableData>> for Headings<HeadersSource>
where
    HeadersSource: HeadersFromData,
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        if let Some(resolved) = &mut self.resolved {
            let level_measure = &mut resolved.level_measure;
            let level_remap = &resolved.level_remap;

            let item_measure = &mut data.measures[self.axis];
            let item_remap = &data.remaps[self.axis];

            match event {
                Event::MouseDown(me) => {
                    let (pix_item, pix_level) = self.axis.pixels_from_point(&me.pos);
                    if let Some(vis_level_idx) = level_measure.vis_idx_from_pixel(pix_level) {
                        if let Some(log_level_idx) = level_remap.get_log_idx(vis_level_idx) {
                            if me.count == 2 && log_level_idx == LogIdx(0) {
                                let extend = me.mods.ctrl() || me.mods.meta();
                                if let Some(vis_idx) = item_measure.vis_idx_from_pixel(pix_item) {
                                    if let Some(log_idx) = data.remaps[self.axis].get_log_idx(vis_idx) {
                                        data.remap_specs[self.axis.cross_axis()].toggle_sort(log_idx, extend);
                                    }
                                    ctx.set_handled()
                                }
                            } else if me.count == 1 {
                                //TODO: Combine lookups?
                                if let Some(idx) = item_measure.pixel_near_border(pix_item) {
                                    if idx > VisIdx(0) && item_measure.can_resize(idx - VisOffset(1)) {
                                        self.resize_dragging = Some(ResizeDragging::new(HeaderAxis::Item, idx - VisOffset(1)));
                                        ctx.set_active(true);
                                        ctx.set_cursor(self.axis.resize_cursor());
                                        ctx.set_handled()
                                    }
                                } else if let Some(idx) = level_measure.pixel_near_border(pix_level) {
                                    if idx > VisIdx(0) && level_measure.can_resize(idx - VisOffset(1)) {
                                        self.resize_dragging = Some(ResizeDragging::new(HeaderAxis::Level, idx - VisOffset(1)));
                                        ctx.set_active(true);
                                        ctx.set_cursor(self.axis.cross_axis().resize_cursor());
                                        ctx.set_handled()
                                    }
                                } else if let Some(idx) = item_measure.vis_idx_from_pixel(pix_item) {
                                    let sel = &mut data.selection;
                                    // Already selected so move headings:
                                    if sel.fully_selects_heading(self.axis, idx) && self.allow_moves {
                                        if let Some(first_px) = item_measure.first_pixel_from_vis(idx) {
                                            self.moving = Some(HeaderMoving::new(
                                                idx,
                                                first_px,
                                                self.axis.pixels_from_point(&me.pos).0,
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
                    }
                }
                Event::MouseMove(me) => {
                    let (pix_main, pix_level) = self.axis.pixels_from_point(&me.pos);
                    let over_idx = item_measure.vis_idx_from_pixel(pix_main);

                    if let Some(resize_dragging) = self.resize_dragging {
                        match resize_dragging.header_axis{
                            HeaderAxis::Item => item_measure.set_far_pixel_for_vis(resize_dragging.idx, pix_main, item_remap),
                            HeaderAxis::Level => level_measure.set_far_pixel_for_vis(resize_dragging.idx, pix_level, level_remap)
                        };

                        if me.buttons.is_empty() {
                            self.resize_dragging = None;
                        } else {
                            ctx.set_cursor(self.axis.resize_cursor());
                        }
                        ctx.request_layout();
                        ctx.set_handled()
                    } else if let Some(moving) = &mut self.moving {
                        moving.current_pos = self.axis.pixels_from_point(&me.pos).0;

                        if let Some(log_idx) = item_remap.get_log_idx(moving.idx) {
                            data.overrides.measure[self.axis]
                                .entry(log_idx)
                                .or_insert_with(|| {
                                    item_measure.pix_range_from_vis(moving.idx).unwrap_or_else(|| {
                                        PixelRange::new(
                                            moving.current_first_px(),
                                            item_measure
                                                .far_pixel_from_vis(moving.idx)
                                                .unwrap_or(moving.current_pos + 100.),
                                        )
                                    })
                                })
                                .move_to(moving.current_first_px());
                            ctx.request_layout();
                        }

                        ctx.request_paint();
                        ctx.set_handled();
                    } else if self.selection_dragging {
                        if let Some(idx) = over_idx {
                            data.selection.extend_in_axis(self.axis, idx, &data.remaps);
                        }
                    } else if let Some(resize_idx) = item_measure.pixel_near_border(pix_main) {
                        if resize_idx > VisIdx(0) {
                            let cursor = if item_measure.can_resize(resize_idx - VisOffset(1)) {
                                self.axis.resize_cursor()
                            } else {
                                &Cursor::NotAllowed
                            };
                            ctx.set_cursor(cursor);
                            ctx.set_handled();
                        }
                    } else if let Some(resize_idx) = level_measure.pixel_near_border(pix_level) {
                        if resize_idx > VisIdx(0) {
                            let cursor = if level_measure.can_resize(resize_idx - VisOffset(1)) {
                                self.axis.cross_axis().resize_cursor()
                            } else {
                                &Cursor::NotAllowed
                            };
                            ctx.set_cursor(cursor);
                            ctx.set_handled();
                        }
                    } else {
                        match over_idx {
                            Some(moving_idx)
                            if data.selection.fully_selects_heading(self.axis, moving_idx)
                                && self.allow_moves =>
                                {
                                    ctx.set_cursor(&Cursor::OpenHand)
                                }
                            _ => ctx.clear_cursor(),
                        }
                    }
                }
                Event::MouseUp(me) => {
                    let (pix_main, pix_level) = self.axis.pixels_from_point(&me.pos);
                    if let Some(resize_dragging) = self.resize_dragging {
                        match resize_dragging.header_axis{
                            HeaderAxis::Item => item_measure.set_far_pixel_for_vis(resize_dragging.idx, pix_main, item_remap),
                            HeaderAxis::Level => level_measure.set_far_pixel_for_vis(resize_dragging.idx, pix_level, level_remap)
                        };
                        self.resize_dragging = None;
                        ctx.request_layout();
                        ctx.set_active(false);
                        ctx.set_handled();
                    } else if let Some(moving) = self.moving.take() {
                        if let Some(log_idx) = item_remap.get_log_idx(moving.idx) {
                            data.overrides.measure[self.axis].remove(&log_idx);
                            ctx.request_layout();
                        }

                        if let Some(moved_to_idx) = item_measure.vis_idx_from_pixel(pix_main) {
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
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            self.refresh_headers(data);
            if self.ensure_pods(data) {
                ctx.children_changed();
                ctx.request_anim_frame();
            }
        }

        if let Some(resolved) = &self.resolved {
            for ((_, log_idx), pod) in &mut self.pods.entries_mut() {
                if let Some(pod) = pod {
                    resolved.headers.with(*log_idx, |header| {
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
        // Todo check data more precisely

        if !old_data.same(data) {
            self.refresh_headers(&data);
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



        let axis = self.axis;
        if let Some(resolved) = &self.resolved {
            let header_measure = &data.measures[axis];
            let remap = &data.remaps[axis];
            let overrides = &data.overrides.measure[axis];
            let pixel_lengths = OverriddenPixelLengths::new(header_measure, &data.remaps[axis], overrides);

            let size = bc.constrain(axis.size(header_measure.total_pixel_length(), // use pixel_lengths
                                              resolved.level_measure.total_pixel_length()));
            let pods = &mut self.pods;


            for (cell, field_span) in &resolved.visible_headings {
                let (log_level, log_field_idx) = cell;
                if let Some(vis_level) = resolved.level_remap.get_vis_idx(*log_level) {
                    if let Some(level_pix) = resolved.level_measure.pix_range_from_vis(vis_level) {

                        if let Some(vis_field_idx) = remap.get_vis_idx(*log_field_idx){
                            if let Some(field_pix) = pixel_lengths.pix_range_from_vis_span(vis_field_idx, VisOffset(*field_span as isize) ) {

                                if let Some(Some(pod)) = pods.get_mut(&cell) {
                                    resolved.headers.with(*log_field_idx, |header| {
                                        if pod.is_initialized() {
                                            let cell_size = axis.size(field_pix.extent(), level_pix.extent());
                                            pod.layout(
                                                ctx,
                                                &BoxConstraints::tight(cell_size).loosen(),
                                                header,
                                                env,
                                            );
                                            let origin = axis.cell_origin(field_pix.p_0, level_pix.p_0);
                                            pod.set_origin(ctx, header, env, origin);
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
            size
        }else{
            bc.min()
        }
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        let axis = self.axis;
        let header_measure = &data.measures[axis];
        let overrides = &data.overrides.measure[axis];

        let remap = &data.remaps[axis];
        let pixel_lengths = OverriddenPixelLengths::new(header_measure, remap, overrides);

        let rc = &data.resolved_config;

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

        ctx.fill(rect, &rc.header_background);

        let (p0, p1) = self.axis.pixels_from_rect(&rect);

        let pods = &mut self.pods;
        if let Some(resolved) = &self.resolved {
            let headers = &resolved.headers;

            for (cell, field_span) in &resolved.visible_headings {
                let (log_level, log_field_idx) = cell;
                if let Some(vis_level) = resolved.level_remap.get_vis_idx(*log_level) {
                    if let Some(level_pix) = resolved.level_measure.pix_range_from_vis(vis_level)
                    {
                        if let Some(vis_field_idx) = remap.get_vis_idx(*log_field_idx){
                            if let Some(field_pix) = pixel_lengths.pix_range_from_vis_span(vis_field_idx, VisOffset(*field_span as isize) ) {

                                let cell_rect = Rect::from_origin_size(
                                    axis.cell_origin(field_pix.p_0, level_pix.p_0),
                                    axis.size(field_pix.extent(), level_pix.extent()),
                                );

                                if indices_selection.vis_index_selected(vis_field_idx) {
                                    ctx.fill(cell_rect, &rc.header_selected_background);
                                }

                                let padded_rect = cell_rect.inset(-rc.cell_padding);

                                let sort_spec = sort_dirs.get(&log_field_idx);

                                headers.with(*log_field_idx, |col_name| {
                                    ctx.with_save(|ctx| {
                                        let clip_rect = if let Some(sort_spec) = sort_spec {
                                            draw_sort_indicator(ctx, sort_spec, padded_rect)
                                        } else {
                                            padded_rect
                                        };
                                        ctx.clip(clip_rect);

                                        if let Some(Some(pod)) = pods.get_mut(cell) {
                                            pod.paint(ctx, col_name, env);
                                        }
                                    });
                                });

                                ctx.stroke_bottom_left_border(
                                    &cell_rect,
                                    &rc.cells_border,
                                    rc.cell_border_thickness,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}


impl<HeadersSource> BindableAccess for Headings<HeadersSource>
where
    HeadersSource: HeadersFromData,
{
    bindable_self_body!();
}


