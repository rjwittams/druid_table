use druid_table::{CellRender, CellRenderExt, CellsDelegate, FixedAxisMeasure, HeaderBuild, HeadersFromIndices, IndexedData, IndexedItems, LogIdx, Remap, RemapSpec, Remapper, SuppliedHeaders, Table, TableArgs, TableConfig, TextCell, CellCtx, EditorFactory};

use druid::{AppLauncher, Color, Data, Env, PaintCtx, Widget, WindowDesc};
use druid_table::numbers_table::LogIdxTable;
use std::marker::PhantomData;

#[macro_use]
extern crate log;

#[derive(Clone)]
struct BigTableCells<TableData: IndexedData, CR: CellRender<TableData::Item>>
where TableData::Item : Data
{
    inner: CR,
    columns: usize,
    phantom_td: PhantomData<TableData>,
}

impl<TableData: IndexedData, CR: CellRender<TableData::Item>>
    BigTableCells<TableData, CR>
where TableData::Item : Data
{
    fn new(inner: CR, columns: usize) -> BigTableCells<TableData, CR> {
        BigTableCells {
            inner,
            columns,
            phantom_td: PhantomData::default(),
        }
    }
}

impl<TableData: IndexedData, CR: CellRender<TableData::Item>>
    CellRender<TableData::Item> for BigTableCells<TableData, CR>
where TableData::Item : Data
{
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.inner.init(ctx, env);
    }

    fn paint(
        &self,
        ctx: &mut PaintCtx,
        cell: &CellCtx,
        data: &TableData::Item,
        env: &Env,
    ) {
        self.inner.paint(ctx, cell, data, env)
    }
}

impl<TableData: IndexedData<Item = LogIdx>, CR: CellRender<LogIdx>> IndexedItems
    for BigTableCells<TableData, CR>
{
    type Item = LogIdx;
    type Idx = LogIdx;

    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        if idx.0 < self.columns {
            Some(f(&idx))
        } else {
            None
        }
    }

    fn with_mut<V>(&mut self, _idx: Self::Idx, _f: impl FnOnce(&mut Self::Item) -> V) -> Option<V> {
        None
    }

    fn idx_len(&self) -> usize {
        self.columns
    }
}



impl<TableData: IndexedData, CR: CellRender<TableData::Item>>
    CellsDelegate<TableData> for BigTableCells<TableData, CR>
where TableData::Item : Data
{
    fn number_of_columns_in_data(&self, _data: &TableData) -> usize {
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

    fn remap(&self, _table_data: &TableData, _remap_spec: &RemapSpec) -> Remap {
        Remap::Pristine
    }
}

impl<CR: CellRender<TableData::Item>, TableData: IndexedData>
EditorFactory<TableData::Item> for BigTableCells<TableData, CR>
where TableData::Item : Data
{
    fn make_editor(&mut self, _ctx: &CellCtx) -> Option<Box<dyn Widget<TableData::Item>>> {
        None
    }
}

fn build_root_widget() -> impl Widget<LogIdxTable> {
    let table_config = TableConfig::new();

    let inner_render = TextCell::new().on_result_of(|br: &LogIdx| br.0.to_string());

    let columns = 1_000_000_000;
    let rows = HeaderBuild::new(
        HeadersFromIndices::new(),
        TextCell::new()
            .text_color(Color::WHITE)
            .on_result_of(|br: &LogIdx| br.0.to_string()),
    );

    let headers = BigTableCells::<LogIdxTable, _>::new(inner_render, columns);
    let cols = HeaderBuild::new(
        SuppliedHeaders::new(headers),
        TextCell::new()
            .text_color(Color::WHITE)
            .on_result_of(|br: &LogIdx| br.0.to_string()),
    );

    let row_m = FixedAxisMeasure::new(25.);
    let col_m = FixedAxisMeasure::new(100.);
    Table::new(TableArgs::new(
        BigTableCells::new(
            TextCell::new().on_result_of(|br: &LogIdx| br.0.to_string()),
            columns,
        ),
        (row_m.clone(), row_m),
        (col_m.clone(), col_m),
        Some(rows),
        Some(cols),
        table_config,
    ))
}

pub fn main() {
    simple_logger::init().unwrap();

    info!("Hello table");

    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title("Big table")
        .window_size((400.0, 700.0));

    // create the initial app state
    let initial_state = LogIdxTable::new(1_000_000_000);

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
