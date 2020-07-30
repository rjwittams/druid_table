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
use crate::config::{ResolvedTableConfig, TableConfig, DEFAULT_HEADER_HEIGHT};
use crate::data::ItemsUse;
use crate::render_ext::RenderContextExt;
use crate::selection::{IndicesSelection, SELECT_INDICES};

pub struct ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    column_measure: ColumnMeasure,
    column_headers: ColumnHeaders,
    column_header_render: Render,
    dragging: Option<usize>,
    selection: IndicesSelection,
    phantom_td: PhantomData<TableData>,
    phantom_ch: PhantomData<ColumnHeader>,
    measure_adjustment_handlers: Vec<Box<AxisMeasureAdjustmentHandler>>,
}

impl<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
    ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
{
    pub fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        column_headers: ColumnHeaders,
        column_header_render: Render,
    ) -> ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure> {
        ColumnHeadings {
            config,
            resolved_config: None,
            column_measure,
            column_headers,
            column_header_render,
            dragging: None,
            selection: IndicesSelection::NoSelection,
            phantom_td: PhantomData::default(),
            phantom_ch: PhantomData::default(),
            measure_adjustment_handlers: Default::default(),
        }
    }

    pub fn add_axis_measure_adjustment_handler(
        &mut self,
        handler: impl Fn(&mut EventCtx, &AxisMeasureAdjustment) + 'static,
    ) {
        self.measure_adjustment_handlers.push(Box::new(handler))
    }

    fn set_column_width(&mut self, ctx: &mut EventCtx, idx: usize, pixel: f64) {
        let width = self.column_measure.set_far_pixel_for_idx(idx, pixel);
        let adjustment = AxisMeasureAdjustment::LengthChanged(TableAxis::Columns, idx, width);
        for handler in &self.measure_adjustment_handlers {
            (handler)(ctx, &adjustment)
        }
        ctx.request_layout();
    }
}

impl<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure> Widget<TableData>
    for ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
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
                if let Some(idx) = self.dragging {
                    self.set_column_width(ctx, idx, me.pos.x);
                    if me.buttons.is_empty() {
                        self.dragging = None;
                    }
                } else {
                    let mut cursor = &Cursor::Arrow;
                    if let Some(idx) = self.column_measure.pixel_near_border(me.pos.x) {
                        if idx > 0 && self.column_measure.can_resize(idx - 1) {
                            cursor = &Cursor::ResizeLeftRight;
                            ctx.set_handled()
                        }
                    }
                    ctx.set_cursor(cursor);
                }
            }
            Event::MouseDown(me) => {
                if let Some(idx) = self.column_measure.pixel_near_border(me.pos.x) {
                    if idx > 0 && self.column_measure.can_resize(idx - 1) {
                        self.dragging = Some(idx - 1)
                    }
                }
            }
            Event::MouseUp(me) => {
                if let Some(idx) = self.dragging {
                    self.set_column_width(ctx, idx, me.pos.x);
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
        _data: &TableData,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(env);
            self.column_measure
                .set_axis_properties(rtc.cell_border_thickness, self.column_headers.len());
            self.resolved_config = Some(rtc);
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &TableData,
        _data: &TableData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableData,
        _env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let height = if let Some(rc) = &self.resolved_config {
            rc.header_height
        } else {
            DEFAULT_HEADER_HEIGHT
        };

        bc.constrain(Size::new(self.column_measure.total_pixel_length(), height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &TableData, env: &Env) {
        if let Some(rtc) = &self.resolved_config {
            let rect = ctx.region().to_rect();

            ctx.fill(rect, &rtc.header_background);

            let selected_border = Color::rgb(0xFF, 0, 0);
            let (start_col, end_col) = self
                .column_measure
                .index_range_from_pixels(rect.x0, rect.x1);

            let header_render = &mut self.column_header_render;

            for col_idx in start_col..=end_col {
                let cell_rect = Rect::from_origin_size(
                    Point::new(
                        self.column_measure
                            .first_pixel_from_index(col_idx)
                            .unwrap_or(0.),
                        0.,
                    ),
                    Size::new(
                        self.column_measure
                            .pixels_length_for_index(col_idx)
                            .unwrap_or(0.),
                        rtc.header_height,
                    ),
                );
                let padded_rect = cell_rect.inset(-rtc.cell_padding);
                self.column_headers.use_item(col_idx, |col_name| {
                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            header_render.paint(ctxt, 0, col_idx, col_name, env);
                        });
                    });
                });
                if self.selection.index_selected(col_idx) {
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
