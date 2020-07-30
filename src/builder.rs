use crate::cell_render::{CellRender, TableColumn, TextCell};
use crate::cells::*;

use crate::axis_measure::{AxisMeasure, FixedAxisMeasure, StoredAxisMeasure, ADJUST_AXIS_MEASURE};
use crate::config::TableConfig;
use crate::data::{ItemsLen, ItemsUse, TableRows};
use crate::headings::ColumnHeadings;
use crate::selection::SELECT_INDICES;
use druid::widget::prelude::*;
use druid::widget::{Align, CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{theme, Data, WidgetExt};
use std::marker::PhantomData;

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellRender<RowData>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
}

impl<RowData: Data, TableData: TableRows<Item = RowData>> Default
    for TableBuilder<RowData, TableData>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<RowData: Data, TableData: TableRows<Item = RowData>> TableBuilder<RowData, TableData> {
    pub fn new() -> TableBuilder<RowData, TableData> {
        TableBuilder {
            table_columns: Vec::<TableColumn<RowData, Box<dyn CellRender<RowData>>>>::new(),
            column_header_render: Box::new(TextCell::new().text_color(theme::PRIMARY_LIGHT)),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
        }
    }

    pub fn with_column<CR: CellRender<RowData> + 'static>(
        mut self,
        header: impl Into<String>,
        cell_render: CR,
    ) -> Self {
        self.add_column(header, cell_render);
        self
    }

    pub fn add_column<CR: CellRender<RowData> + 'static>(
        &mut self,
        header: impl Into<String>,
        cell_render: CR,
    ) {
        self.table_columns
            .push(TableColumn::new(header.into(), Box::new(cell_render)));
    }

    pub fn build_widget(self) -> Align<TableData> {
        let column_headers: Vec<String> = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

        let column_measure = StoredAxisMeasure::new(100.);
        let row_measure = FixedAxisMeasure::new(30.);
        build_table(
            column_headers,
            self.table_columns,
            row_measure,
            column_measure,
            self.column_header_render,
            self.table_config,
        )
    }
}

pub fn build_table<
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    RowMeasure: AxisMeasure + 'static,
    ColumnHeader: Data,
    ColumnMeasure: AxisMeasure + 'static,
    ColumnHeaders: ItemsUse<Item = ColumnHeader> + 'static,
    ColumnHeaderRender: CellRender<ColumnHeader> + 'static,
    CellAreaRender: CellRender<RowData> + ItemsLen + 'static,
>(
    column_headers: ColumnHeaders,
    cell_area_render: CellAreaRender,
    row_measure: RowMeasure,
    column_measure: ColumnMeasure,
    column_header_render: ColumnHeaderRender,
    table_config: TableConfig,
) -> Align<TableData> {
    let column_headers_id = WidgetId::next();
    let column_headers_scroll_id = WidgetId::next();
    let cells_id = WidgetId::next();

    let mut headings = ColumnHeadings::new(
        table_config.clone(),
        column_measure.clone(),
        column_headers,
        column_header_render,
    );
    headings.add_axis_measure_adjustment_handler(move |ctx, adj| {
        ctx.submit_command(ADJUST_AXIS_MEASURE.with(*adj), cells_id);
    });

    let ch_scroll =
        Scroll::new(headings.with_id(column_headers_id)).with_id(column_headers_scroll_id);
    let mut cells = Cells::new(table_config, column_measure, row_measure, cell_area_render);
    cells.add_selection_handler(move |ctx, table_sel| {
        let column_sel = table_sel.to_column_selection();
        ctx.submit_command(SELECT_INDICES.with(column_sel), column_headers_id);
    });

    let mut cells_scroll = Scroll::new(cells.with_id(cells_id));
    cells_scroll.add_scroll_handler(move |ctx, pos| {
        ctx.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), column_headers_scroll_id);
    });

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(ch_scroll)
        .with_flex_child(cells_scroll, 1.)
        .center()
}
