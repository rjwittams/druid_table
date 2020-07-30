use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{
    AxisMeasure, AxisMeasureAdjustment, AxisMeasureAdjustmentHandler, TableAxis,
};
use crate::cell_render::CellRender;
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::ItemsUse;
use crate::numbers_table::NumbersTable;
use crate::render_ext::RenderContextExt;
use crate::selection::{IndicesSelection, SELECT_INDICES};
use crate::ItemsLen;

pub trait HeadersFromData<TableData, Header: Data, Headers: ItemsUse<Item = Header>> {
    fn get_headers(&self, table_data: &TableData) -> Headers;
}

impl<TableData, Header, Headers>
    HeadersFromData<TableData, Header, Headers> for Headers where
    Header: Data, Headers: ItemsUse<Item = Header> + Clone
{
    fn get_headers(&self, _table_data: &TableData) -> Headers {
        (*self).clone()
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

impl<TableData: ItemsLen> HeadersFromData<TableData, usize, NumbersTable>
    for HeadersFromIndices<TableData>
{
    fn get_headers(&self, table_data: &TableData) -> NumbersTable {
        NumbersTable::new(table_data.len())
    }
}

pub struct Headings<TableData, Header, Headers, HeadersSource, Render, Measure>
where
    TableData: Data,
    Header: Data,
    Headers: ItemsUse<Item = Header>,
    HeadersSource: HeadersFromData<TableData, Header, Headers>,
    Render: CellRender<Header>,
    Measure: AxisMeasure,
{
    axis: TableAxis,
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    measure: Measure,
    headers_source: HeadersSource,
    headers: Option<Headers>,
    header_render: Render,
    dragging: Option<usize>,
    selection: IndicesSelection,
    phantom_td: PhantomData<TableData>,
    phantom_h: PhantomData<Header>,
    measure_adjustment_handlers: Vec<Box<AxisMeasureAdjustmentHandler>>,
}

impl<TableData, Header, Headers, HeadersSource, Render, Measure>
    Headings<TableData, Header, Headers, HeadersSource, Render, Measure>
where
    TableData: Data,
    Header: Data,
    Headers: ItemsUse<Item = Header>,
    HeadersSource: HeadersFromData<TableData, Header, Headers>,
    Render: CellRender<Header>,
    Measure: AxisMeasure,
{
    pub fn new(
        axis: TableAxis,
        config: TableConfig,
        measure: Measure,
        headers_source: HeadersSource,
        header_render: Render,
    ) -> Headings<TableData, Header, Headers, HeadersSource, Render, Measure> {
        Headings {
            axis,
            config,
            resolved_config: None,
            measure,
            headers_source,
            headers: None,
            header_render,
            dragging: None,
            selection: IndicesSelection::NoSelection,
            phantom_td: PhantomData::default(),
            phantom_h: PhantomData::default(),
            measure_adjustment_handlers: Default::default(),
        }
    }

    pub fn add_axis_measure_adjustment_handler(
        &mut self,
        handler: impl Fn(&mut EventCtx, &AxisMeasureAdjustment) + 'static,
    ) {
        self.measure_adjustment_handlers.push(Box::new(handler))
    }

    fn set_pix_length_for_axis(&mut self, ctx: &mut EventCtx, idx: usize, pixel: f64) {
        let length = self.measure.set_far_pixel_for_idx(idx, pixel);
        let adjustment = AxisMeasureAdjustment::LengthChanged(self.axis, idx, length);
        for handler in &self.measure_adjustment_handlers {
            (handler)(ctx, &adjustment)
        }
        ctx.request_layout();
    }
}

impl<TableData, Header, Headers, HeadersSource, Render, Measure> Widget<TableData>
    for Headings<TableData, Header, Headers, HeadersSource, Render, Measure>
where
    TableData: Data,
    Header: Data,
    Headers: ItemsUse<Item = Header>,
    HeadersSource: HeadersFromData<TableData, Header, Headers>,
    Render: CellRender<Header>,
    Measure: AxisMeasure,
{
    fn event(&mut self, ctx: &mut EventCtx, _event: &Event, _data: &mut TableData, _env: &Env) {
        match _event {
            Event::Command(ref cmd) => {
                if cmd.is(SELECT_INDICES) {
                    if let Some(index_selections) = cmd.get(SELECT_INDICES) {
                        self.selection = index_selections.clone();
                        ctx.request_paint()
                    }
                }
            }
            Event::MouseMove(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.dragging {

                    self.set_pix_length_for_axis(ctx, idx,
                                                 pix_main);
                    if me.buttons.is_empty() {
                        self.dragging = None;
                    }
                } else {
                    let mut cursor = &Cursor::Arrow;
                    if let Some(idx) = self.measure.pixel_near_border(pix_main) {
                        if idx > 0 && self.measure.can_resize(idx - 1) {
                            cursor = self.axis.resize_cursor();
                            ctx.set_handled()
                        }
                    }
                    ctx.set_cursor(cursor);
                }
            }
            Event::MouseDown(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.measure.pixel_near_border(pix_main) {
                    if idx > 0 && self.measure.can_resize(idx - 1) {
                        self.dragging = Some(idx - 1)
                    }
                }
            }
            Event::MouseUp(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.dragging {
                    self.set_pix_length_for_axis(ctx, idx, pix_main);
                    self.dragging = None;
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableData,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(env);
            self.headers = Some(self.headers_source.get_headers(data)); // TODO Option
            self.measure.set_axis_properties(
                rtc.cell_border_thickness,
                self.headers.as_ref().unwrap().len(),
            );
            self.resolved_config = Some(rtc);
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &TableData, data: &TableData, _env: &Env) {
        if let Some(rtc) = &self.resolved_config {
            if !old_data.same(data) {
                self.headers = Some(self.headers_source.get_headers(data));
                self.measure.set_axis_properties(
                    rtc.cell_border_thickness,
                    self.headers.as_ref().unwrap().len(),
                );
                ctx.request_layout();
            }
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableData,
        _env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let cross_axis_length = if let Some(rc) = &self.resolved_config {
            rc.header_height
        } else {
            self.axis.default_header_cross()
        };

        bc.constrain(
            self.axis
                .size(self.measure.total_pixel_length(), cross_axis_length),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &TableData, env: &Env) {
        if let (Some(rtc), Some(headers)) = (&self.resolved_config, &self.headers) {
            let rect = ctx.region().to_rect();

            ctx.fill(rect, &rtc.header_background);

            let selected_border = Color::rgb(0xFF, 0, 0);
            let (p0, p1) = self.axis.pixels_from_rect(&rect);
            let (start_main, end_main) = self.measure.index_range_from_pixels(p0, p1);

            let header_render = &mut self.header_render;

            for main_idx in start_main..=end_main {
                let first_pix = self.measure.first_pixel_from_index(main_idx).unwrap_or(0.);
                let length_pix = self.measure.pixels_length_for_index(main_idx).unwrap_or(0.);
                let origin = self.axis.cell_origin(first_pix, 0.);
                Point::new(first_pix, 0.);
                let size = self.axis.size(length_pix, rtc.header_height /*TODO */);
                let cell_rect = Rect::from_origin_size(origin, size);
                let padded_rect = cell_rect.inset(-rtc.cell_padding);
                headers.use_item(main_idx, |col_name| {
                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            header_render.paint(ctxt, 0, main_idx, col_name, env);
                        });
                    });
                });
                if self.selection.index_selected(main_idx) {
                    ctx.stroke(padded_rect, &selected_border, 2.);
                } else {
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
