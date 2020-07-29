use std::fmt::Debug;

use druid_table::{CellRenderExt, TableConfig, TableRows, TextCell};

use druid::{AppLauncher, Data, Lens, Widget, WindowDesc};

#[macro_use]
extern crate log;

#[derive(Debug, Data, Clone, Lens)]
struct BigRow {
    row: usize,
    row_str: String,
}

#[derive(Debug, Data, Clone, Lens)]
struct BigTable {
    rows: usize,
}

impl TableRows<BigRow> for BigTable {
    fn len(&self) -> usize {
        return self.rows;
    }

    fn use_row<V>(&self, idx: usize, f: impl FnOnce(&BigRow) -> V) -> Option<V> {
        if idx < self.rows {
            let temp = BigRow {
                row: idx,
                row_str: idx.to_string(),
            };
            Some(f(&temp))
        } else {
            None
        }
    }
}

fn build_root_widget() -> impl Widget<BigTable> {
    let mut table_config = TableConfig::<BigRow, BigTable>::new();

    for idx in 0..20 {
        table_config.add_column(
            format!("Col {:?}", idx),
            TextCell::new().lens(BigRow::row_str),
        );
    }

    table_config.build_widget()
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
