use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{AxisMeasureAdjustment, AxisMeasureAdjustmentHandler, LogIdx, TableAxis, VisIdx, VisOffset, AxisMeasureE};
use crate::columns::{CellCtx, CellRender};
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{IndexedItems, SortSpec};
use crate::numbers_table::LogIdxTable;
use crate::render_ext::RenderContextExt;
use crate::table::TableState;
use druid::widget::Bindable;
use std::collections::HashMap;

pub trait HeadersFromData {
    type TableData: Data;
    type Header: Data;
    type Headers: IndexedItems<Item = Self::Header, Idx = LogIdx>;
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

impl<Headers: IndexedItems<Idx = LogIdx> + Clone, TableData: Data> HeadersFromData
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

impl<TableData: IndexedItems + Data> HeadersFromData for HeadersFromIndices<TableData> {
    type TableData = TableData;
    type Header = LogIdx;
    type Headers = LogIdxTable;

    fn get_headers(&self, table_data: &TableData) -> LogIdxTable {
        LogIdxTable::new(table_data.idx_len())
    }
}

pub struct Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>
{
    axis: TableAxis,
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    measure: AxisMeasureE,
    headers_source: HeadersSource,
    headers: Option<HeadersSource::Headers>,
    header_render: Render,
    resize_dragging: Option<VisIdx>,
    selection_dragging: bool,
    measure_adjustment_handlers: Vec<Box<AxisMeasureAdjustmentHandler>>
}

impl<HeadersSource, Render> Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>
{
    pub fn new(
        axis: TableAxis,
        config: TableConfig,
        measure: AxisMeasureE,
        headers_source: HeadersSource,
        header_render: Render,
    ) -> Headings<HeadersSource, Render> {
        Headings {
            axis,
            config,
            resolved_config: None,
            measure,
            headers_source,
            headers: None,
            header_render,
            resize_dragging: None,
            selection_dragging: false,
            measure_adjustment_handlers: Default::default()
        }
    }

    pub fn add_axis_measure_adjustment_handler(
        &mut self,
        handler: impl Fn(&mut EventCtx, &AxisMeasureAdjustment) + 'static,
    ) {
        self.measure_adjustment_handlers.push(Box::new(handler))
    }

    fn set_pix_length_for_axis(&mut self, ctx: &mut EventCtx, vis_idx: VisIdx, pixel: f64) {
        let length = self.measure.set_far_pixel_for_vis(vis_idx, pixel); //TODO Jam calls together with richer result?

        let adjustment = AxisMeasureAdjustment::LengthChanged(self.axis, vis_idx, length);
        for handler in &self.measure_adjustment_handlers {
            (handler)(ctx, &adjustment)
        }

        // TODO : this might be overkill if we knew that we are bigger that the viewport - repaint would work
        ctx.request_layout();
    }
}

impl<HeadersSource, Render> Widget<TableState<HeadersSource::TableData>>
    for Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        _event: &Event,
        data: &mut TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        match _event {
            Event::MouseDown(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if me.count == 2 {
                    let extend = me.mods.ctrl() || me.mods.meta();
                    if let Some(vis_idx) = self.measure.vis_idx_from_pixel(pix_main) {
                        if let Some(log_idx) = data.remaps[&self.axis].get_log_idx(vis_idx) {
                            data.remap_specs[&self.axis.cross_axis()].toggle_sort(log_idx, extend);
                        }
                        ctx.set_handled()
                    }
                } else if me.count == 1 {
                    //TODO: Combine lookups
                    if let Some(idx) = self.measure.pixel_near_border(pix_main) {
                        if idx > VisIdx(0) && self.measure.can_resize(idx - VisOffset(1)) {
                            self.resize_dragging = Some(idx - VisOffset(1));
                            ctx.set_active(true);
                            ctx.set_cursor(self.axis.resize_cursor());
                            ctx.set_handled()
                        }
                    } else if let Some(idx) = self.measure.vis_idx_from_pixel(pix_main) {
                        let sel = &mut data.selection;
                        if me.mods.shift(){
                            sel.extend_in_axis(&self.axis, idx, &data.remaps);
                        }else {
                            sel.select_in_axis(&self.axis, idx, &data.remaps);
                        }
                        self.selection_dragging = true;
                        ctx.set_active(true);
                        ctx.set_handled()
                    }
                }
            },
            Event::MouseMove(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.resize_dragging {
                    self.set_pix_length_for_axis(ctx, idx, pix_main);

                    if me.buttons.is_empty() {
                        self.resize_dragging = None;
                    } else {
                        ctx.set_cursor(self.axis.resize_cursor());
                    }
                    ctx.set_handled()
                } else if self.selection_dragging {
                    if let Some(idx) = self.measure.vis_idx_from_pixel(pix_main) {
                        data.selection.extend_in_axis(&self.axis, idx, &data.remaps);
                    }
                } else if let Some(idx) = self.measure.pixel_near_border(pix_main) {
                    if idx > VisIdx(0) {
                        let cursor = if self.measure.can_resize(idx - VisOffset(1)) {
                            self.axis.resize_cursor()
                        } else {
                            &Cursor::NotAllowed
                        };
                        ctx.set_handled();
                        ctx.set_cursor(cursor);
                        ctx.set_handled();
                    }
                }
            },
            Event::MouseUp(me) => {
                if let Some(idx) = self.resize_dragging {
                    let pix_main = self.axis.main_pixel_from_point(&me.pos);
                    self.set_pix_length_for_axis(ctx, idx, pix_main);
                    self.resize_dragging = None;
                    ctx.set_active(false);
                    ctx.set_handled();
                }
                if self.selection_dragging {
                    self.selection_dragging = false;
                    ctx.set_active(false);
                    ctx.set_handled()
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(env);
            self.headers = Some(self.headers_source.get_headers(&data.data)); // TODO Option
            self.resolved_config = Some(rtc);
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
            self.headers = Some(self.headers_source.get_headers(&data.data));
            ctx.request_layout(); // TODO Only relayout if actually changed
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableState<HeadersSource::TableData>,
        _env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let cross_axis_length = if let Some(rc) = &self.resolved_config {
            match self.axis {
                TableAxis::Columns => rc.col_header_height,
                TableAxis::Rows => rc.row_header_width,
            }
        } else {
            self.axis.default_header_cross()
        };

        bc.constrain(
            self.axis
                .size(self.measure.total_pixel_length(), cross_axis_length),
        )
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        let indices_selection = data.selection.to_axis_selection(&self.axis, &data.remaps);

        // TODO build on change of spec
        let cross_rem = &data.remap_specs[self.axis.cross_axis()];
        let sort_dirs: HashMap<LogIdx, SortSpec> = cross_rem
            .sort_by
            .iter()
            .enumerate()
            .map(|(ord, x)| (LogIdx(x.idx), SortSpec::new(ord, x.direction)))
            .collect();

        if let (Some(rtc), Some(headers)) = (&self.resolved_config, &self.headers) {
            self.header_render.init(ctx, env);
            let rect = ctx.region().to_rect();

            ctx.fill(rect, &rtc.header_background);

            let (p0, p1) = self.axis.pixels_from_rect(&rect);
            let (start_main, end_main) = self.measure.vis_range_from_pixels(p0, p1);

            let header_render = &mut self.header_render;

            for vis_main_idx in VisIdx::range_inc_iter(start_main, end_main) {
                let first_pix = self
                    .measure
                    .first_pixel_from_vis(vis_main_idx)
                    .unwrap_or(0.);
                let length_pix = self
                    .measure
                    .pixels_length_for_vis(vis_main_idx)
                    .unwrap_or(0.);
                let axis = self.axis;
                let origin = axis.cell_origin(first_pix, 0.);
                Point::new(first_pix, 0.);
                let size = axis.size(length_pix, rtc.cross_axis_length(&axis));
                let cell_rect = Rect::from_origin_size(origin, size);


                if indices_selection.vis_index_selected(vis_main_idx) {
                    ctx.fill(cell_rect, &rtc.header_selected_background);
                }
                let padded_rect = cell_rect.inset(-rtc.cell_padding);
                if let Some(log_main_idx) = data.remaps[&self.axis].get_log_idx(vis_main_idx) {
                    headers.with(log_main_idx, |col_name| {
                        ctx.with_save(|ctx| {
                            let layout_origin = padded_rect.origin().to_vec2();
                            ctx.clip(padded_rect);
                            ctx.transform(Affine::translate(layout_origin));
                            ctx.with_child_ctx(padded_rect, |ctxt| {
                                let cell = CellCtx::Header(
                                    &axis,
                                    log_main_idx,
                                    sort_dirs.get(&log_main_idx),
                                );
                                header_render.paint(ctxt, &cell, col_name, env);
                            });
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
}

impl<HeadersSource, Render> Bindable for Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>
{
}
