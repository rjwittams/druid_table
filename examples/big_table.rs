use druid_table::{
    build_table, AxisBuild, CellRender, CellRenderExt, FixedAxisMeasure, HeadersFromIndices,
    ItemsLen, ItemsUse, Remap, RemapSpec, Remapper, SuppliedHeaders, TableConfig, TableRows,
    TextCell,
};

use druid::{AppLauncher, Color, Data, Env, PaintCtx, Widget, WindowDesc};
use druid_table::numbers_table::NumbersTable;
use std::marker::PhantomData;

#[macro_use]
extern crate log;

#[derive(Clone)]
struct ManyColumns<T, CR: CellRender<T>> {
    inner: CR,
    columns: usize,
    phantom_t: PhantomData<T>,
}

impl<T, CR: CellRender<T>> ManyColumns<T, CR> {
    fn new(inner: CR, columns: usize) -> ManyColumns<T, CR> {
        ManyColumns {
            inner,
            columns,
            phantom_t: PhantomData::default(),
        }
    }
}

impl<T, CR: CellRender<T>> CellRender<T> for ManyColumns<T, CR> {
    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.inner.paint(ctx, row_idx, col_idx, data, env)
    }
}

impl<T, CR: CellRender<T>> ItemsLen for ManyColumns<T, CR> {
    fn len(&self) -> usize {
        self.columns
    }
}

impl<CR: CellRender<usize>> ItemsUse for ManyColumns<usize, CR> {
    type Item = usize;

    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        if idx < self.columns {
            Some(f(&idx))
        } else {
            None
        }
    }
}

impl<RowData: Data, CR: CellRender<RowData>, TableData: TableRows<Item = RowData>>
    Remapper<RowData, TableData> for ManyColumns<RowData, CR>
{
    fn sort_fixed(&self, idx: usize) -> bool {
        true
    }

    fn initial_spec(&self) -> RemapSpec {
        RemapSpec::default()
    }

    fn remap(&self, _table_data: &TableData, _remap_spec: &RemapSpec) -> Remap {
        Remap::Pristine
    }
}

fn build_root_widget() -> impl Widget<NumbersTable> {
    let table_config = TableConfig::new();

    let inner_render = TextCell::new().on_result_of(|br: &usize| br.to_string());

    let columns = 1_000_000_000;
    let rows = AxisBuild::new(
        HeadersFromIndices::new(),
        FixedAxisMeasure::new(25.),
        TextCell::new()
            .text_color(Color::WHITE)
            .on_result_of(|br: &usize| br.to_string()),
    );

    let cols = AxisBuild::new(
        SuppliedHeaders::new(ManyColumns::new(inner_render, columns)),
        FixedAxisMeasure::new(100.),
        TextCell::new()
            .text_color(Color::WHITE)
            .on_result_of(|br: &usize| br.to_string()),
    );

    build_table(
        ManyColumns::new(
            TextCell::new().on_result_of(|br: &usize| br.to_string()),
            columns,
        ),
        rows,
        cols,
        table_config,
    )
}

pub fn main() {
    simple_logger::init().unwrap();

    info!("Hello table");

    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title("Big table")
        .window_size((400.0, 700.0));

    // create the initial app state
    let initial_state = NumbersTable::new(1_000_000_000);

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
