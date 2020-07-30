use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{Affine, BoxConstraints, Color, Data, Env, Event, EventCtx,
            LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
            Size, UpdateCtx, Widget};


use crate::cell_render::{CellRender};
use crate::data::{ItemsLen, TableRows};
use crate::selection::{TableSelection, SelectionHandler, SingleCell};
use crate::axis_measure::{AxisMeasure, AxisMeasureAdjustment, TableAxis, ADJUST_AXIS_MEASURE};
use crate::config::{TableConfig, ResolvedTableConfig};
use crate::render_ext::RenderContextExt;



pub struct Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: Data,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    cell_render: Render,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

impl<RowData, TableData, Render, RowMeasure, ColumnMeasure>
    Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: Data,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    pub fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        row_measure: RowMeasure,
        cell_render: Render,
    ) -> Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure> {
        Cells {
            config,
            resolved_config: None,
            selection_handlers: Vec::new(),
            selection: TableSelection::NoSelection,
            column_measure,
            row_measure,
            cell_render,
            phantom_rd: PhantomData::default(),
            phantom_td: PhantomData::default(),
        }
    }

    pub fn add_selection_handler(
        &mut self,
        selection_handler: impl Fn(&mut EventCtx, &TableSelection) + 'static,
    ) {
        self.selection_handlers.push(Box::new(selection_handler));
    }

    fn set_selection(&mut self, ctx: &mut EventCtx, selection: TableSelection) {
        self.selection = selection;
        for sh in &self.selection_handlers {
            sh(ctx, &self.selection)
        }
        ctx.request_paint();
    }

    fn find_cell(&self, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            self.row_measure.index_from_pixel(pos.y),
            self.column_measure.index_from_pixel(pos.x),
        );
        Some(SingleCell::new(r?, c?))
    }
}

impl<RowData, TableData, Render, RowMeasure, ColumnMeasure> Widget<TableData>
    for Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut TableData, _env: &Env) {
        let mut new_selection: Option<TableSelection> = None;

        match event {
            Event::MouseDown(me) => {
                if let Some(cell) = self.find_cell(&me.pos) {
                    new_selection = Some(cell.into())
                    // TODO: Modifier keys ask current selection to add this cell
                }
            }
            Event::Command(cmd) => {
                if cmd.is(ADJUST_AXIS_MEASURE){
                   if let Some(AxisMeasureAdjustment::LengthChanged(TableAxis::Columns, idx, length)) = cmd.get(ADJUST_AXIS_MEASURE){
                       self.column_measure.set_pixel_length_for_idx(*idx, *length);
                       ctx.request_layout();
                   }
                }
            }
            _=>()
        }

        if let Some(sel) = new_selection {
            self.set_selection(ctx, sel);
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &TableData,
        _env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(_env);
            self.column_measure
                .set_axis_properties(rtc.cell_border_thickness, self.cell_render.len());
            self.row_measure
                .set_axis_properties(rtc.cell_border_thickness, _data.len());
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
        bc.debug_check("TableCells");
        bc.constrain(Size::new(
            self.column_measure.total_pixel_length(),
            self.row_measure.total_pixel_length(),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &rtc.cells_background);

        let (start_row, end_row) = self.row_measure.index_range_from_pixels(rect.y0, rect.y1);
        let (start_col, end_col) = self
            .column_measure
            .index_range_from_pixels(rect.x0, rect.x1);

        for row_idx in start_row..=end_row {
            let row_top = self.row_measure.first_pixel_from_index(row_idx);

            data.use_item(row_idx, |row| {
                for col_idx in start_col..=end_col {
                    let cell_left = self.column_measure.first_pixel_from_index(col_idx);
                    let selected = (&self.selection).get_cell_status(row_idx, col_idx);

                    let cell_rect = Rect::from_origin_size(
                        Point::new(cell_left.unwrap_or(0.), row_top.unwrap_or(0.)),
                        Size::new(
                            self.column_measure.pixels_length_for_index(col_idx).unwrap_or(0.),
                            self.row_measure.pixels_length_for_index(row_idx).unwrap_or(0.),
                        ),
                    );
                    let padded_rect = cell_rect.inset(-rtc.cell_padding);

                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            self.cell_render.paint(ctxt, row_idx, col_idx, row, env);
                        });
                    });

                    if selected.into() {
                        ctx.stroke(
                            cell_rect,
                            &Color::rgb(0, 0, 0xFF),
                            rtc.cell_border_thickness,
                        );
                    } else {
                        ctx.stroke_bottom_left_border(
                            &cell_rect,
                            &rtc.cells_border,
                            rtc.cell_border_thickness,
                        );
                    }
                }
            });
        }
    }
}


