use crate::columns::{CellDelegate, ProvidedColumns, TableColumn};

use crate::axis_measure::{AxisMeasure, AxisPair, LogIdx, TableAxis};
use crate::config::TableConfig;
use crate::data::{IndexedData, IndexedDataDiffer, RefreshDiffer};
use crate::headings::{HeadersFromIndices, SuppliedHeaders, StaticHeader};
use crate::{DisplayFactory, HeaderBuild, ReadOnly, Table, WidgetCell};
use druid::lens::Identity;
use druid::{theme, Data, KeyOrValue};
use im::Vector;
use std::marker::PhantomData;

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

pub struct TableBuilder<ColumnHeader, TableData: IndexedData> {
    table_columns: Vec<TableColumn<ColumnHeader, TableData::Item, Box<dyn CellDelegate<TableData::Item>>>>,
    column_header_delegate: Box<dyn DisplayFactory<ColumnHeader>>,
    row_header_delegate: Box<dyn DisplayFactory<LogIdx>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
    show_headings: ShowHeadings,
    measurements: AxisPair<AxisMeasurementType>,
    differ: Option<Box<dyn IndexedDataDiffer<TableData>>>,
}

#[derive(Debug, Copy, Clone, Data, Ord, PartialOrd, Eq, PartialEq)]
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

impl<TableData: IndexedData> TableBuilder<String, TableData> {
    pub fn new() -> Self {
        TableBuilder {
            table_columns: Vec::new(),
            row_header_delegate: Box::new(WidgetCell::text_configured(
                |rl| rl.with_text_color(theme::LABEL_COLOR),
                || ReadOnly::new(|br: &LogIdx| br.0.to_string()),
            )),
            column_header_delegate: Box::new(WidgetCell::text_configured(
                |rl| rl.with_text_color(theme::LABEL_COLOR),
                || Identity,
            )),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
            show_headings: ShowHeadings::Both,
            measurements: AxisPair::new(
                AxisMeasurementType::Individual,
                AxisMeasurementType::Individual,
            ),
            differ: None,
        }
    }
}

impl<ColumnHeader, TableData: IndexedData> TableBuilder<ColumnHeader, TableData> {
    pub fn new_custom_col(column_header_delegate: impl DisplayFactory<ColumnHeader> + 'static) -> Self {
        TableBuilder {
            table_columns: Vec::new(),
            row_header_delegate: Box::new(WidgetCell::text_configured(
                |rl| rl.with_text_color(theme::LABEL_COLOR),
                || ReadOnly::new(|br: &LogIdx| br.0.to_string()),
            )),
            column_header_delegate: Box::new(column_header_delegate),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
            show_headings: ShowHeadings::Both,
            measurements: AxisPair::new(
                AxisMeasurementType::Individual,
                AxisMeasurementType::Individual,
            ),
            differ: None,
        }
    }
}


impl<Header: Data, TableData: IndexedData> TableBuilder<Header, TableData> {
    pub fn diff_with(mut self, differ: impl IndexedDataDiffer<TableData> + 'static) -> Self {
        self.differ = Some(Box::new(differ));
        self
    }

    pub fn border(mut self, thickness: impl Into<KeyOrValue<f64>>) -> Self {
        self.table_config.cell_border_thickness = thickness.into();
        self
    }

    pub fn headings(mut self, show_headings: ShowHeadings) -> Self {
        self.show_headings = show_headings;
        self
    }

    pub fn with(
        mut self,
        col: TableColumn<Header, TableData::Item, Box<dyn CellDelegate<TableData::Item>>>,
    ) -> Self {
        self.table_columns.push(col);
        self
    }

    pub fn with_column<CD: CellDelegate<TableData::Item> + 'static>(
        mut self,
        header: impl Into<Header>,
        cell_delegate: CD,
    ) -> Self {
        self.add_column(header, cell_delegate);
        self
    }

    pub fn add_column<CD: CellDelegate<TableData::Item> + 'static>(
        &mut self,
        header: impl Into<Header>,
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

    pub fn build(mut self) -> Table<TableData> where Header : StaticHeader {
        let measures = self.build_measures();
        let Self {
            show_headings,
            table_columns,
            row_header_delegate,
            column_header_delegate,
            differ,
            table_config,
            ..
        } = self;

        let column_headers: Vector<_> = table_columns.iter().map(|tc| tc.header.clone()).collect();

        let row_build = show_headings.should_show(&TableAxis::Rows).then(|| {
            HeaderBuild::new(
                HeadersFromIndices::default(),
                row_header_delegate,
            )
        });

        let col_build = show_headings.should_show(&TableAxis::Columns).then(|| {
            HeaderBuild::new(SuppliedHeaders::new(column_headers), column_header_delegate)
        });

        let differ = differ.unwrap_or(Box::new(RefreshDiffer));
        Table::new(
            ProvidedColumns::new(table_columns),
            row_build,
            col_build,
            table_config,
            measures,
            differ,
        )
    }
}
