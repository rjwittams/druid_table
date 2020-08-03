use crate::cells::*;
use crate::columns::{
    CellDelegate, CellRender, CellRenderExt, DataCompare, ProvidedColumns, TableColumn, TextCell,
};

use crate::axis_measure::{AxisMeasure, LogIdx, StoredAxisMeasure, TableAxis, ADJUST_AXIS_MEASURE};
use crate::config::TableConfig;
use crate::data::{IndexedData, IndexedItems, Remap, RemapDetails, RemapSpec, Remapper, SortSpec};
use crate::headings::{HeadersFromData, HeadersFromIndices, Headings, SuppliedHeaders};
use crate::numbers_table::LogIdxTable;
use crate::selection::{SELECT_INDICES, CellAddress};
use crate::table::{HeaderBuildT, TableArgs};
use crate::{HeaderBuild, Table};
use druid::widget::prelude::*;
use druid::widget::{Align, CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{theme, Data, WidgetExt};
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::cell::Cell;

#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum AxisMeasurements{
    Fixed,
    Adjustable
}

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellDelegate<RowData>>>>,
    column_header_delegate: Box<dyn CellDelegate<String>>,
    row_header_delegate: Box<dyn CellDelegate<LogIdx>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
    show_headings: ShowHeadings,
    measurements: CellAddress<AxisMeasurements>
}

impl<RowData: Data, TableData: IndexedData<Item = RowData, Idx = LogIdx>> Default
    for TableBuilder<RowData, TableData>
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Data, Ord, PartialOrd, Eq, PartialEq)]
pub enum ShowHeadings {
    Both,
    One(TableAxis),
    JustCells,
}

impl ShowHeadings {
    fn should_show(&self, a: &TableAxis) -> bool {
        match self {
            Self::Both => true,
            Self::JustCells => false,
            Self::One(ta) => ta == a,
        }
    }
}

pub type DefaultTableArgs<TableData: IndexedData<Idx=LogIdx> > = TableArgs<
    TableData,
    StoredAxisMeasure,
    StoredAxisMeasure,
    HeaderBuild<HeadersFromIndices<TableData>, Box<dyn CellDelegate<LogIdx>>>,
    HeaderBuild<SuppliedHeaders<Vec<String>, TableData>, Box<dyn CellDelegate<String>>>,
    ProvidedColumns<TableData, Box<dyn CellDelegate<<TableData as IndexedItems>::Item>>>>;

impl<RowData: Data, TableData: IndexedData<Item = RowData, Idx = LogIdx>>
    TableBuilder<RowData, TableData>
{
    pub fn new() -> TableBuilder<RowData, TableData> {
        TableBuilder {
            table_columns: Vec::<TableColumn<RowData, Box<dyn CellDelegate<RowData>>>>::new(),
            row_header_delegate: Box::new(
                TextCell::new()
                    .text_color(theme::PRIMARY_LIGHT)
                    .on_result_of(|br: &LogIdx| br.0.to_string()),
            ),
            column_header_delegate: Box::new(TextCell::new().text_color(theme::PRIMARY_LIGHT)),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
            show_headings: ShowHeadings::Both,
            measurements: CellAddress::new(AxisMeasurements::Adjustable, AxisMeasurements::Adjustable)
        }
    }

    pub fn headings(mut self, show_headings: ShowHeadings) -> Self {
        self.show_headings = show_headings;
        self
    }

    pub fn with(mut self, col: TableColumn<RowData, Box<dyn CellDelegate<RowData>>>) -> Self {
        self.table_columns.push(col);
        self
    }

    pub fn with_column<CD: CellDelegate<RowData> + 'static>(
        mut self,
        header: impl Into<String>,
        cell_delegate: CD,
    ) -> Self {
        self.add_column(header, cell_delegate);
        self
    }

    pub fn add_column<CD: CellDelegate<RowData> + 'static>(
        &mut self,
        header: impl Into<String>,
        cell_render: CD,
    ) {
        self.table_columns
            .push(TableColumn::new(header, Box::new(cell_render)));
    }

    pub fn measuring(mut self, axis: TableAxis, measure: AxisMeasurements)->Self{
        self.measurements[axis] = measure;
        self
    }

    pub fn build_args(self) -> DefaultTableArgs<TableData> {
        let column_headers: Vec<String> = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

        let column_measure = StoredAxisMeasure::new(100.);
        let row_measure = StoredAxisMeasure::new(30.);

        let row_build = if_opt!(
            self.show_headings.should_show(&TableAxis::Rows),
            HeaderBuild::new(
                HeadersFromIndices::<TableData>::new(),
                self.row_header_delegate,
            )
        );
        let col_build = if_opt!(
            self.show_headings.should_show(&TableAxis::Columns),
            HeaderBuild::new(
                SuppliedHeaders::new(column_headers),
                self.column_header_delegate,
            )
        );

        TableArgs::new(
            ProvidedColumns::new(self.table_columns),
            row_measure,
            column_measure,
            row_build,
            col_build,
            self.table_config,
        )
    }
}
