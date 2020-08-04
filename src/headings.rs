use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, Selector, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{
    AxisMeasure, AxisMeasureAdjustment, AxisMeasureAdjustmentHandler, LogIdx, TableAxis, VisIdx,
    VisOffset,
};
use crate::columns::CellRender;
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::IndexedItems;
use crate::numbers_table::LogIdxTable;
use crate::render_ext::RenderContextExt;
use crate::selection::{IndicesSelection, SELECT_INDICES};
use crate::Remap;

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

#[derive(Debug, Copy, Clone)]
pub enum HeaderSection {
    Main,
    // Filter
}

#[derive(Debug, Copy, Clone)]
pub struct HeaderClicked(pub TableAxis, pub VisIdx, pub HeaderSection);

pub const HEADER_CLICKED: Selector<HeaderClicked> =
    Selector::new("druid-builtin.table.header-clicked");

pub type HeaderClickedHandler = dyn Fn(&mut EventCtx, &MouseEvent, &HeaderClicked);

pub struct Headings<HeadersSource, Render, Measure>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
    Measure: AxisMeasure,
{
    axis: TableAxis,
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    measure: Measure,
    headers_source: HeadersSource,
    headers: Option<HeadersSource::Headers>,
    header_render: Render,
    dragging: Option<VisIdx>,
    selection: IndicesSelection,
    measure_adjustment_handlers: Vec<Box<AxisMeasureAdjustmentHandler>>,
    header_clicked_handlers: Vec<Box<HeaderClickedHandler>>,
}

impl<HeadersSource, Render, Measure> Headings<HeadersSource, Render, Measure>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
    Measure: AxisMeasure,
{
    pub fn new(
        axis: TableAxis,
        config: TableConfig,
        measure: Measure,
        headers_source: HeadersSource,
        header_render: Render,
    ) -> Headings<HeadersSource, Render, Measure> {
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
            measure_adjustment_handlers: Default::default(),
            header_clicked_handlers: Default::default(),
        }
    }

    pub fn add_axis_measure_adjustment_handler(
        &mut self,
        handler: impl Fn(&mut EventCtx, &AxisMeasureAdjustment) + 'static,
    ) {
        self.measure_adjustment_handlers.push(Box::new(handler))
    }

    pub fn add_header_clicked_handler(
        &mut self,
        handler: impl Fn(&mut EventCtx, &MouseEvent, &HeaderClicked) + 'static,
    ) {
        self.header_clicked_handlers.push(Box::new(handler))
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

impl<HeadersSource, Render, Measure> Widget<HeadersSource::TableData>
    for Headings<HeadersSource, Render, Measure>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
    Measure: AxisMeasure,
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut HeadersSource::TableData,
        _env: &Env,
    ) {
        match _event {
            Event::Command(ref cmd) => {
                if cmd.is(SELECT_INDICES) {
                    if let Some(index_selections) = cmd.get(SELECT_INDICES) {
                        self.selection = index_selections.clone();
                        ctx.request_paint();
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseMove(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.dragging {
                    self.set_pix_length_for_axis(ctx, idx, pix_main);

                    if me.buttons.is_empty() {
                        self.dragging = None;
                    } else {
                        ctx.set_cursor(self.axis.resize_cursor());
                    }
                    ctx.set_handled()
                } else {
                    if let Some(idx) = self.measure.pixel_near_border(pix_main) {
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
                }
            }
            Event::MouseDown(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                //TODO: Combine lookups
                if let Some(idx) = self.measure.pixel_near_border(pix_main) {
                    if idx > VisIdx(0) && self.measure.can_resize(idx - VisOffset(1)) {
                        self.dragging = Some(idx - VisOffset(1));
                        ctx.set_active(true);
                        ctx.set_cursor(self.axis.resize_cursor());
                    }
                } else if let Some(idx) = self.measure.vis_idx_from_pixel(pix_main) {
                    // TODO will need to remember/recreate text extents and other decorations in the cells
                    // to handle more than just Main
                    let clicked = HeaderClicked(self.axis, idx, HeaderSection::Main);
                    for handler in &self.header_clicked_handlers {
                        handler(ctx, me, &clicked);
                    }
                    ctx.set_handled()
                }
            }
            Event::MouseUp(me) => {
                if let Some(idx) = self.dragging {
                    let pix_main = self.axis.main_pixel_from_point(&me.pos);
                    self.set_pix_length_for_axis(ctx, idx, pix_main);
                    self.dragging = None;
                    ctx.set_active(false);
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &HeadersSource::TableData,
        env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded => {
                let rtc = self.config.resolve(env);
                self.headers = Some(self.headers_source.get_headers(data)); // TODO Option
                self.measure.set_axis_properties(
                    rtc.cell_border_thickness,
                    self.headers.as_ref().unwrap().idx_len(),
                    &Remap::Pristine, // TODO: Column reordering..
                );
                self.resolved_config = Some(rtc);
            }
            // LifeCycle::HotChanged(false) => {
            //     self.dragging = None;
            // }
            _ => {}
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &HeadersSource::TableData,
        data: &HeadersSource::TableData,
        _env: &Env,
    ) {
        if let Some(rtc) = &self.resolved_config {
            if !old_data.same(data) {
                self.headers = Some(self.headers_source.get_headers(data));
                self.measure.set_axis_properties(
                    rtc.cell_border_thickness,
                    self.headers.as_ref().unwrap().idx_len(),
                    &Remap::Pristine, // TODO: Column reordering..
                );
                ctx.request_layout();
            }
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &HeadersSource::TableData,
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

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &HeadersSource::TableData, env: &Env) {
        if let (Some(rtc), Some(headers)) = (&self.resolved_config, &self.headers) {
            self.header_render.init(ctx, env);
            let rect = ctx.region().to_rect();

            ctx.fill(rect, &rtc.header_background);

            let selected_border = Color::rgb(0xFF, 0, 0);
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
                let origin = self.axis.cell_origin(first_pix, 0.);
                Point::new(first_pix, 0.);
                let size = self
                    .axis
                    .size(length_pix, rtc.cross_axis_length(&self.axis));
                let cell_rect = Rect::from_origin_size(origin, size);
                let padded_rect = cell_rect.inset(-rtc.cell_padding);
                if let Some(log_main_idx) = Remap::Pristine.get_log_idx(vis_main_idx) {
                    // TODO: use proper remap
                    headers.with(log_main_idx, |col_name| {
                        ctx.with_save(|ctx| {
                            let layout_origin = padded_rect.origin().to_vec2();
                            ctx.clip(padded_rect);
                            ctx.transform(Affine::translate(layout_origin));
                            ctx.with_child_ctx(padded_rect, |ctxt| {
                                //TODO: These indexes are wrong but not used
                                header_render.paint(ctxt, LogIdx(0), log_main_idx, col_name, env);
                            });
                        });
                    });
                    if self.selection.vis_index_selected(vis_main_idx) {
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
}
