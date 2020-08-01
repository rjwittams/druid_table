use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{AxisMeasure, AxisMeasureAdjustment, TableAxis, ADJUST_AXIS_MEASURE};
use crate::columns::CellRender;
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{
    ItemsLen, RemapSpec, RemappedItems, Remapper, SortDirection, SortSpec, TableRows,
};
use crate::render_ext::RenderContextExt;
use crate::selection::{SelectionHandler, SingleCell, TableSelection};
use crate::{ItemsUse, Remap};

pub trait ColumnsBehaviour<RowData: Data, TableData: TableRows<Item = RowData>>:
    CellRender<RowData> + ItemsLen + Remapper<RowData, TableData>
{
}

impl<RowData: Data, TableData: TableRows<Item = RowData>, T> ColumnsBehaviour<RowData, TableData>
    for T
where
    T: CellRender<RowData> + ItemsLen + Remapper<RowData, TableData>,
{
}

pub struct Cells<RowData, TableData, ColBehaviour, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    ColBehaviour: ColumnsBehaviour<RowData, TableData>, // The length is the number of columns
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    columns: ColBehaviour,
    remap_spec_rows: RemapSpec,
    remap_rows: Remap,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
    Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    ColDel: ColumnsBehaviour<RowData, TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    pub fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        row_measure: RowMeasure,
        columns: ColDel,
    ) -> Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> {
        Cells {
            config,
            resolved_config: None,
            selection_handlers: Vec::new(),
            selection: TableSelection::NoSelection,
            column_measure,
            row_measure,
            remap_spec_rows: RemapSpec::default(),
            remap_rows: Remap::Pristine,
            columns,
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

    fn paint_cells(
        &self,
        ctx: &mut PaintCtx,
        data: &impl ItemsUse<Item = RowData>,
        env: &Env,
        rtc: &ResolvedTableConfig,
        start_row: usize,
        end_row: usize,
        start_col: usize,
        end_col: usize,
    ) {
        for row_idx in start_row..=end_row {
            let row_top = self.row_measure.first_pixel_from_index(row_idx);

            data.use_item(row_idx, |row| {
                self.paint_row(ctx, env, &rtc, start_col, end_col, row_idx, row_top, row)
            });
        }
    }

    fn paint_row(
        &self,
        ctx: &mut PaintCtx,
        env: &Env,
        rtc: &ResolvedTableConfig,
        start_col: usize,
        end_col: usize,
        row_idx: usize,
        row_top: Option<f64>,
        row: &RowData,
    ) {
        for col_idx in start_col..=end_col {
            let cell_left = self.column_measure.first_pixel_from_index(col_idx);
            let selected = (&self.selection).get_cell_status(row_idx, col_idx);

            let cell_rect = Rect::from_origin_size(
                Point::new(cell_left.unwrap_or(0.), row_top.unwrap_or(0.)),
                Size::new(
                    self.column_measure
                        .pixels_length_for_index(col_idx)
                        .unwrap_or(0.),
                    self.row_measure
                        .pixels_length_for_index(row_idx)
                        .unwrap_or(0.),
                ),
            );
            let padded_rect = cell_rect.inset(-rtc.cell_padding);

            ctx.with_save(|ctx| {
                let layout_origin = padded_rect.origin().to_vec2();
                ctx.clip(padded_rect);
                ctx.transform(Affine::translate(layout_origin));
                ctx.with_child_ctx(padded_rect, |ctxt| {
                    self.columns.paint(ctxt, row_idx, col_idx, row, env);
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
    }
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> Widget<TableData>
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    ColDel: ColumnsBehaviour<RowData, TableData>,
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
                if cmd.is(ADJUST_AXIS_MEASURE) {
                    if let Some(AxisMeasureAdjustment::LengthChanged(axis, idx, length)) =
                        cmd.get(ADJUST_AXIS_MEASURE)
                    {
                        match axis {
                            TableAxis::Rows => {
                                self.row_measure.set_pixel_length_for_idx(*idx, *length)
                            }
                            TableAxis::Columns => {
                                self.column_measure.set_pixel_length_for_idx(*idx, *length)
                            }
                        };
                        ctx.request_layout();
                    }
                }
            }
            _ => (),
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
                .set_axis_properties(rtc.cell_border_thickness, self.columns.len());
            self.row_measure
                .set_axis_properties(rtc.cell_border_thickness, _data.len());
            self.resolved_config = Some(rtc);
            self.remap_spec_rows = self.columns.initial_spec();
            self.remap_rows = self.columns.remap(_data, &self.remap_spec_rows);
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
        self.columns.init(ctx, env); // TODO reduce calls? Invalidate on some changes

        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &rtc.cells_background);

        let (start_row, end_row) = self.row_measure.index_range_from_pixels(rect.y0, rect.y1);
        let (start_col, end_col) = self
            .column_measure
            .index_range_from_pixels(rect.x0, rect.x1);

        match &self.remap_rows {
            Remap::Selected(details) => {
                let details_copy = details;
                let items = RemappedItems::new(data, &details_copy);
                self.paint_cells(
                    ctx, &items, env, &rtc, start_row, end_row, start_col, end_col,
                )
            }
            _ => self.paint_cells(ctx, data, env, &rtc, start_row, end_row, start_col, end_col),
        }
    }
}
