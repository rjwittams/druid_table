use std::fmt::Debug;

use druid_table::{
    build_table, CellRender, CellRenderExt, FixedSizeAxis, ItemsLen, ItemsUse, TableConfig,
    TextCell,
};

use druid::{AppLauncher, Color, Data, Env, Lens, PaintCtx, Widget, WindowDesc};
use std::marker::PhantomData;

#[macro_use]
extern crate log;

#[derive(Debug, Data, Clone, Lens)]
struct BigRow {
    row: usize,
}

#[derive(Debug, Data, Clone, Lens)]
struct BigTable {
    rows: usize,
}

impl ItemsLen for BigTable {
    fn len(&self) -> usize {
        return self.rows;
    }
}

impl ItemsUse for BigTable {
    type Item = BigRow;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&BigRow) -> V) -> Option<V> {
        if idx < self.rows {
            let temp = BigRow { row: idx };
            Some(f(&temp))
        } else {
            None
        }
    }
}

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
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
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

fn build_root_widget() -> impl Widget<BigTable> {
    let table_config = TableConfig::new();

    let inner_render = TextCell::new()
        .on_result_of(|br: &usize| br.to_string())
        .lens(BigRow::row);

    let columns = 1_000_000_000;
    build_table(
        ManyColumns::new(
            TextCell::new().on_result_of(|br: &usize| br.to_string()),
            columns,
        ),
        ManyColumns::new(inner_render, columns),
        FixedSizeAxis::new(25.),
        FixedSizeAxis::new(100.),
        TextCell::new()
            .text_color(Color::WHITE)
            .on_result_of(|br: &usize| br.to_string()),
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
    let initial_state = BigTable {
        rows: 1_000_000_000,
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
