
use crate::columns::{
    CellDelegate,  CellRenderExt, ProvidedColumns, TableColumn, TextCell, HeaderCell
};

use crate::axis_measure::{
    AxisMeasure, AxisPair, LogIdx, StoredAxisMeasure, TableAxis,
};
use crate::config::TableConfig;
use crate::data::{IndexedData, IndexedItems};
use crate::headings::{HeadersFromIndices, SuppliedHeaders};
use crate::table::{TableArgs};
use crate::{FixedAxisMeasure, HeaderBuild, CellRender};
use druid::{theme, Data, KeyOrValue};
use std::cell::{RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum AxisMeasurementType {
    Uniform,
    Individual, /* O(n) in memory with number of items on the axis */
}

impl Default for AxisMeasurementType {
    fn default() -> Self {
        AxisMeasurementType::Individual
    }
}

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellDelegate<RowData>>>>,
    column_header_delegate: Box<dyn CellRender<String>>,
    row_header_delegate: Box<dyn CellRender<LogIdx>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
    show_headings: ShowHeadings,
    measurements: AxisPair<AxisMeasurementType>,
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

type DynAxisMeasure = Rc<RefCell<dyn AxisMeasure>>;

pub type DefaultTableArgs<TableData> = TableArgs<
    TableData,
    DynAxisMeasure,
    DynAxisMeasure,
    HeaderBuild<HeadersFromIndices<TableData>, Box<dyn CellRender<LogIdx>>>,
    HeaderBuild<SuppliedHeaders<Vec<String>, TableData>, Box<dyn CellRender<String>>>,
    ProvidedColumns<TableData, Box<dyn CellDelegate<<TableData as IndexedItems>::Item>>>,
>;

impl<RowData: Data, TableData: IndexedData<Item = RowData, Idx = LogIdx>>
    TableBuilder<RowData, TableData>
{
    pub fn new() -> TableBuilder<RowData, TableData> {
        TableBuilder {
            table_columns: Vec::<TableColumn<RowData, Box<dyn CellDelegate<RowData>>>>::new(),
            row_header_delegate: Box::new(
                HeaderCell::new(TextCell::new()
                    .text_color(theme::LABEL_COLOR))
                    .on_result_of(|br: &LogIdx| br.0.to_string()),
            ),
            column_header_delegate: Box::new(HeaderCell::new(TextCell::new().text_color(theme::LABEL_COLOR))),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
            show_headings: ShowHeadings::Both,
            measurements: AxisPair::new(AxisMeasurementType::Individual, AxisMeasurementType::Individual),
        }
    }

    pub fn border(mut self, thickness: impl Into<KeyOrValue<f64>>) -> Self {
        self.table_config.cell_border_thickness = thickness.into();
        self
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

    pub fn measuring_axis(mut self, axis: &TableAxis, measure: AxisMeasurementType) -> Self {
        self.measurements[axis] = measure;
        self
    }

    pub fn build_measure(&self, axis: &TableAxis, size: f64) -> DynAxisMeasure {
        match self.measurements[axis] {
            AxisMeasurementType::Individual =>{
                Rc::new(RefCell::new(StoredAxisMeasure::new(size)))
            },
            AxisMeasurementType::Uniform =>{
                Rc::new(RefCell::new(FixedAxisMeasure::new(size)))
            },
        }
    }

    pub fn build_args(self) -> DefaultTableArgs<TableData> {
        let column_headers: Vec<String> = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

        let column_measure = self.build_measure(&TableAxis::Columns, 100.);
        let row_measure = self.build_measure(&TableAxis::Rows, 30.);

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
            (row_measure.clone(), row_measure),
            (column_measure.clone(), column_measure),
            row_build,
            col_build,
            self.table_config,
        )
    }
}
