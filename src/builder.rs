use crate::columns::{
    CellDelegate, CellRenderExt, ProvidedColumns, TableColumn, TextCell,
};

use crate::axis_measure::{AxisMeasure, AxisPair, LogIdx, TableAxis};
use crate::config::TableConfig;
use crate::data::{IndexedData};
use crate::headings::{HeadersFromIndices, SuppliedHeaders};
use crate::table::TableArgs;
use crate::{CellRender, HeaderBuild, Table};
use druid::{theme, Data, KeyOrValue};
use std::marker::PhantomData;
use im::Vector;
use druid::lens::Map;
use crate::vis::MarkShape::Text;

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

pub struct TableBuilder<TableData: IndexedData> {
    table_columns: Vec<TableColumn<TableData::Item, Box<dyn CellDelegate<TableData::Item>>>>,
    column_header_delegate: Box<dyn CellRender<String>>,
    row_header_delegate: Box<dyn CellRender<LogIdx>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
    show_headings: ShowHeadings,
    measurements: AxisPair<AxisMeasurementType>,
}

impl<TableData: IndexedData> Default
    for TableBuilder<TableData>
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

pub type DefaultTableArgs<TableData> = TableArgs<
    TableData,
    HeaderBuild<HeadersFromIndices<TableData>, Box<dyn CellRender<LogIdx>>>,
    HeaderBuild<SuppliedHeaders<Vector<String>, TableData>, Box<dyn CellRender<String>>>,
    ProvidedColumns<TableData, Box<dyn CellDelegate<<TableData as IndexedData>::Item>>>,
>;

impl<TableData: IndexedData>
    TableBuilder<TableData>
{
    pub fn new() -> TableBuilder<TableData> {
        TableBuilder {
            table_columns: Vec::new(),
            row_header_delegate: Box::new(
                //HeaderCell::new(TextCell::new().text_color(theme::LABEL_COLOR))
                TextCell::new().text_color(theme::LABEL_COLOR)
                    .lens( Map::new(|br: &LogIdx| br.0.to_string(),
                                    |br: &mut LogIdx, a: String|{} ) )

            ),
            column_header_delegate: Box::new(//HeaderCell::new(
                TextCell::new().text_color(theme::LABEL_COLOR),
            //)
            ),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
            show_headings: ShowHeadings::Both,
            measurements: AxisPair::new(
                AxisMeasurementType::Individual,
                AxisMeasurementType::Individual,
            ),
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

    pub fn with(mut self, col: TableColumn<TableData::Item, Box<dyn CellDelegate<TableData::Item>>>) -> Self {
        self.table_columns.push(col);
        self
    }

    pub fn with_column<CD: CellDelegate<TableData::Item> + 'static>(
        mut self,
        header: impl Into<String>,
        cell_delegate: CD,
    ) -> Self {
        self.add_column(header, cell_delegate);
        self
    }

    pub fn add_column<CD: CellDelegate<TableData::Item> + 'static>(
        &mut self,
        header: impl Into<String>,
        cell_render: CD,
    ) {
        self.table_columns
            .push(TableColumn::new(header, Box::new(cell_render)));
    }

    pub fn measuring_axis(mut self, axis: TableAxis, measure: AxisMeasurementType) -> Self {
        self.measurements[axis] = measure;
        self
    }

    fn build_measure(&self, axis: TableAxis, size: f64) -> AxisMeasure {
        AxisMeasure::new(self.measurements[axis], size)
    }

    fn build_measures(&self) -> AxisPair<AxisMeasure> {
        AxisPair::new(
            self.build_measure(TableAxis::Rows, 30.),
            self.build_measure(TableAxis::Columns, 100.),
        )
    }

    fn build_args(self) -> DefaultTableArgs<TableData> {
        let column_headers: Vector<String> = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

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
            row_build,
            col_build,
            self.table_config,
        )
    }

    pub fn build(self) -> Table<TableData> {
        let measures = self.build_measures();
        Table::new(self.build_args(), measures)
    }
}
