use druid_table::{AxisMeasure, AxisMeasurementType, AxisPair, CellCtx, CellRender, CellRenderExt, CellsDelegate, EditorFactory, HeaderBuild, HeadersFromIndices, IndexedData, LogIdx, Remap, RemapSpec, Remapper, SuppliedHeaders, Table, TableArgs, TableConfig, TextCell, ReadOnly};

use druid::{AppLauncher, Color, Data, Env, PaintCtx, Widget, WindowDesc, EventCtx, Event};
use druid_table::numbers_table::LogIdxTable;
use std::marker::PhantomData;
use std::fmt::{Debug, Formatter};
use core::fmt;
use druid::lens::Map;

#[macro_use]
extern crate log;

#[derive(Data, Clone)]
struct BigTableCols
{
    columns: usize
}

impl BigTableCols {
    pub fn new(columns: usize) -> Self {
        BigTableCols { columns }
    }
}


#[derive(Clone)]
struct BigTableCells<TableData: IndexedData, CR: CellRender<TableData::Item>>
where
    TableData::Item: Data,
{
    inner: CR,
    columns: usize,
    phantom_td: PhantomData<TableData>,
}

impl <TableData: IndexedData, CR: CellRender<TableData::Item>> Debug for BigTableCells<TableData, CR>
    where
        TableData::Item: Data{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("BigTableCells").finish()
    }
}

impl<TableData: IndexedData, CR: CellRender<TableData::Item>> BigTableCells<TableData, CR>
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

impl<TableData: IndexedData, CR: CellRender<TableData::Item>> CellRender<TableData::Item>
    for BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.inner.init(ctx, env);
    }

    fn paint(&self, ctx: &mut PaintCtx, cell: &CellCtx, data: &TableData::Item, env: &Env) {
        self.inner.paint(ctx, cell, data, env)
    }

    fn event(&self, ctx: &mut EventCtx, cell: &CellCtx, event: &Event, data: &mut <TableData as IndexedData>::Item, env: &Env) {

    }

    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.inner.make_display(cell)
    }
}

impl IndexedData for BigTableCols
{
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

impl<TableData: IndexedData, CR: CellRender<TableData::Item>> CellsDelegate<TableData>
    for BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn data_columns(&self, _data: &TableData) -> usize {
        self.columns
    }
}

impl<RowData: Data, CR: CellRender<RowData>, TableData: IndexedData<Item = RowData>>
    Remapper<TableData> for BigTableCells<TableData, CR>
{
    fn sort_fixed(&self, _idx: usize) -> bool {
        true
    }

    fn initial_spec(&self) -> RemapSpec {
        RemapSpec::default()
    }

    fn remap_items(&self, _table_data: &TableData, _remap_spec: &RemapSpec) -> Remap {
        Remap::Pristine
    }
}

impl<CR: CellRender<TableData::Item>, TableData: IndexedData> EditorFactory<TableData::Item>
    for BigTableCells<TableData, CR>
where
    TableData::Item: Data,
{
    fn make_editor(&self, _ctx: &CellCtx) -> Option<Box<dyn Widget<TableData::Item>>> {
        None
    }
}

fn build_root_widget() -> Table<LogIdxTable> {
    let table_config = TableConfig::new();

    let rows = HeaderBuild::new(
        HeadersFromIndices::new(),
        TextCell::new()
            .text_color(Color::WHITE)
            .lens(ReadOnly::new(|br: &LogIdx| br.0.to_string()))
    );

    let columns = 1_000_000_000;
    let headers = BigTableCols::new(columns);
    let cols = HeaderBuild::new(
        SuppliedHeaders::new(headers),
        TextCell::new()
            .text_color(Color::WHITE)
            .lens(ReadOnly::new(|br: &LogIdx| br.0.to_string()))
    );

    let measures = AxisPair::new(
        AxisMeasure::new(AxisMeasurementType::Uniform, 25.),
        AxisMeasure::new(AxisMeasurementType::Uniform, 100.),
    );
    Table::new(
        TableArgs::new(
            BigTableCells::new(
                TextCell::new().lens(ReadOnly::new(|br: &LogIdx| br.0.to_string())),
                columns,
            ),
            Some(rows),
            Some(cols),
            table_config,
        ),
        measures,
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
