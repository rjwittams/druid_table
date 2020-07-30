use crate::cell_render::{CellRender, CellRenderExt, TableColumn, TextCell};
use crate::cells::*;

use crate::axis_measure::{
    AxisMeasure, FixedAxisMeasure, StoredAxisMeasure, TableAxis, ADJUST_AXIS_MEASURE,
};
use crate::config::TableConfig;
use crate::data::{ItemsLen, ItemsUse, TableRows};
use crate::headings::{HeadersFromData, HeadersFromIndices, Headings};
use crate::selection::SELECT_INDICES;
use druid::widget::prelude::*;
use druid::widget::{Align, CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{theme, Data, WidgetExt};
use std::marker::PhantomData;

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellRender<RowData>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    row_header_render: Box<dyn CellRender<usize>>,
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
            row_header_render: Box::new(
                TextCell::new()
                    .text_color(theme::PRIMARY_LIGHT)
                    .on_result_of(|br: &usize| br.to_string()),
            ),
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
        let row_measure = StoredAxisMeasure::new(30.);

        let row_build = AxisBuild::new(
            HeadersFromIndices::new(),
            row_measure,
            self.row_header_render,
        );
        let col_build = AxisBuild::new(column_headers, column_measure, self.column_header_render);

        build_table(self.table_columns, row_build, col_build, self.table_config)
    }
}

pub struct AxisBuild<
    TableData,
    Header: Data,
    Measure: AxisMeasure + 'static,
    Headers: ItemsUse<Item = Header> + 'static,
    HeadersSource: HeadersFromData<TableData, Header, Headers> + 'static,
    HeaderRender: CellRender<Header> + 'static,
> {
    headers_source: HeadersSource,
    measure: Measure,
    header_render: HeaderRender,
    p_td: PhantomData<TableData>,
    p_h: PhantomData<Header>,
    p_hs: PhantomData<Headers>,
}

impl<
        TableData,
        Header: Data,
        Measure: AxisMeasure + 'static,
        Headers: ItemsUse<Item = Header> + 'static,
        HeadersSource: HeadersFromData<TableData, Header, Headers> + 'static,
        HeaderRender: CellRender<Header> + 'static,
    > AxisBuild<TableData, Header, Measure, Headers, HeadersSource, HeaderRender>
{
    pub fn new(
        headers_source: HeadersSource,
        measure: Measure,
        header_render: HeaderRender,
    ) -> Self {
        AxisBuild {
            headers_source,
            measure,
            header_render,
            p_td: Default::default(),
            p_h: Default::default(),
            p_hs: Default::default(),
        }
    }
}

pub fn build_table<
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    RowHeader: Data,
    RowMeasure: AxisMeasure + 'static,
    RowHeaders: ItemsUse<Item = RowHeader> + 'static,
    RowHeadersSource: HeadersFromData<TableData, RowHeader, RowHeaders> + 'static,
    RowHeaderRender: CellRender<RowHeader> + 'static,
    ColumnHeader: Data,
    ColumnMeasure: AxisMeasure + 'static,
    ColumnHeaders: ItemsUse<Item = ColumnHeader> + 'static,
    ColumnHeadersSource: HeadersFromData<TableData, ColumnHeader, ColumnHeaders> + 'static,
    ColumnHeaderRender: CellRender<ColumnHeader> + 'static,
    CellAreaRender: CellRender<RowData> + ItemsLen + 'static,
>(
    cell_area_render: CellAreaRender,
    row: AxisBuild<TableData, RowHeader, RowMeasure, RowHeaders, RowHeadersSource, RowHeaderRender>,
    col: AxisBuild<
        TableData,
        ColumnHeader,
        ColumnMeasure,
        ColumnHeaders,
        ColumnHeadersSource,
        ColumnHeaderRender,
    >,
    table_config: TableConfig,
) -> Align<TableData> {
    let column_headers_id = WidgetId::next();
    let column_scroll_id = WidgetId::next();
    let cells_id = WidgetId::next();
    let row_headers_id = WidgetId::next();
    let row_scroll_id = WidgetId::next();

    let mut col_headings = Headings::new(
        TableAxis::Columns,
        table_config.clone(),
        col.measure.clone(),
        col.headers_source,
        col.header_render,
    );
    col_headings.add_axis_measure_adjustment_handler(move |ctx, adj| {
        ctx.submit_command(ADJUST_AXIS_MEASURE.with(*adj), cells_id);
    });

    let ch_scroll = Scroll::new(col_headings.with_id(column_headers_id)).with_id(column_scroll_id);

    let mut cells = Cells::new(
        table_config.clone(),
        col.measure,
        row.measure.clone(),
        cell_area_render,
    );
    cells.add_selection_handler(move |ctx, table_sel| {
        ctx.submit_command(
            SELECT_INDICES.with(table_sel.to_axis_selection(TableAxis::Columns)),
            column_headers_id,
        );
        ctx.submit_command(
            SELECT_INDICES.with(table_sel.to_axis_selection(TableAxis::Rows)),
            row_headers_id,
        );
    });

    let mut cells_scroll = Scroll::new(cells.with_id(cells_id));
    cells_scroll.add_scroll_handler(move |ctx, pos| {
        ctx.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), column_scroll_id);
        ctx.submit_command(SCROLL_TO.with(ScrollTo::y(pos.y)), row_scroll_id);
    });

    let cells_column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(ch_scroll)
        .with_flex_child(cells_scroll, 1.);

    let mut row_headings = Headings::new(
        TableAxis::Rows,
        table_config.clone(),
        row.measure,
        row.headers_source,
        row.header_render,
    );

    row_headings.add_axis_measure_adjustment_handler(move |ctx, adj| {
        ctx.submit_command(ADJUST_AXIS_MEASURE.with(*adj), cells_id);
    });

    let row_scroll = Scroll::new(row_headings.with_id(row_headers_id)).with_id(row_scroll_id);

    let rh_col = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacer(table_config.header_height)
        .with_flex_child(row_scroll, 1.);

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(rh_col)
        .with_flex_child(cells_column, 1.)
        .center()
    // Todo wrap in top level widget to handle reconfiguration?
}
