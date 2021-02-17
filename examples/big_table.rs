use druid_table::{AxisMeasure, AxisMeasurementType, AxisPair, CellCtx, CellsDelegate, DisplayFactory, HeaderBuild, HeadersFromIndices, IndexedData, LogIdx, ReadOnly, RefreshDiffer, Remap, RemapSpec, Remapper, SuppliedHeaders, Table, TableConfig, WidgetCell, Headers};

use core::fmt;
use druid::lens::Map;
use druid::{AppLauncher, Color, Data, Env, Event, EventCtx, PaintCtx, Widget, WindowDesc};
use druid_table::numbers_table::LogIdxTable;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use druid_bindings::BindableProperty;
use std::cmp::Ordering;

#[macro_use]
extern crate log;

#[derive(Data, Clone)]
struct BigTableCols {
    columns: usize,
}

impl BigTableCols {
    pub fn new(columns: usize) -> Self {
        BigTableCols { columns }
    }
}

#[derive(Clone)]
struct BigTableCells<TableData: IndexedData, CR: DisplayFactory<TableData::Item>>
where
    TableData::Item: Data,
{
    inner: CR,
    columns: usize,
    phantom_td: PhantomData<TableData>,
}

impl Headers for BigTableCols {
    fn header_levels(&self) -> usize {
        1
    }

    fn header_compare(&self, level: LogIdx, a: &Self::Item, b: &Self::Item) -> Ordering {
        a.cmp(b)
    }
}

impl<TableData: IndexedData, CR: DisplayFactory<TableData::Item>> Debug
    for BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("BigTableCells").finish()
    }
}

impl<TableData: IndexedData, CR: DisplayFactory<TableData::Item>> BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn new(inner: CR, columns: usize) -> BigTableCells<TableData, CR> {
        BigTableCells {
            inner,
            columns,
            phantom_td: PhantomData::default(),
        }
    }
}

impl<TableData: IndexedData, CR: DisplayFactory<TableData::Item>> DisplayFactory<TableData::Item>
    for BigTableCells<TableData, CR>
{
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<TableData::Item>>> {
        self.inner.make_display(cell)
    }

    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<TableData::Item>>> {
        None
    }
}

impl IndexedData for BigTableCols {
    type Item = LogIdx;

    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        if idx.0 < self.columns {
            Some(f(&idx))
        } else {
            None
        }
    }

    fn with_mut<V>(&mut self, _idx: LogIdx, _f: impl FnOnce(&mut Self::Item) -> V) -> Option<V> {
        None
    }

    fn data_len(&self) -> usize {
        self.columns
    }
}

impl<TableData: IndexedData, CR: DisplayFactory<TableData::Item>> CellsDelegate<TableData>
    for BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn data_fields(&self, _data: &TableData) -> usize {
        self.columns
    }
}

impl<RowData: Data, CR: DisplayFactory<RowData>, TableData: IndexedData<Item = RowData>>
    Remapper<TableData> for BigTableCells<TableData, CR>
{
    fn sort_fixed(&self, _idx: usize) -> bool {
        true
    }

    fn initial_spec(&self) -> RemapSpec {
        RemapSpec::default()
    }

    fn remap_from_records(&self, _table_data: &TableData, _remap_spec: &RemapSpec) -> Remap {
        Remap::Pristine(_table_data.data_len())
    }
}

fn build_root_widget() -> Table<LogIdxTable> {
    let table_config = TableConfig::new();
    let num_columns = 1_000_000_000;

    let rows = HeaderBuild::new(
        HeadersFromIndices::default(),
        Box::new(WidgetCell::text_configured(
            |rl| rl.with_text_color(Color::WHITE),
            || ReadOnly::new(|br: &LogIdx| br.0.to_string()),
        )),
    );

    let cols = HeaderBuild::new(
        SuppliedHeaders::new(BigTableCols::new(num_columns)),
        Box::new(WidgetCell::text_configured(
            |rl| rl.with_text_color(Color::WHITE),
            || ReadOnly::new(|br: &LogIdx| br.0.to_string()),
        )),
    );

    let measures = AxisPair::new(
        AxisMeasure::new(AxisMeasurementType::Uniform, 25.),
        AxisMeasure::new(AxisMeasurementType::Uniform, 100.),
    );
    Table::new(
        BigTableCells::new(
            WidgetCell::text(|| ReadOnly::new(|br: &LogIdx| br.0.to_string())),
            num_columns,
        ),
        Some(rows),
        Some(cols),
        table_config,
        measures,
        Box::new(RefreshDiffer),
    )
}

pub fn main() {
    simple_logger::init().unwrap();

    info!("Hello table");

    // describe the main window
    let main_window = WindowDesc::new(build_root_widget())
        .title("Big table")
        .window_size((400.0, 700.0));

    // create the initial app state
    let initial_state = LogIdxTable::new(1_000_000_000);

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
