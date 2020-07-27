use std::fmt::Debug;
use druid_table::*;

use druid::widget::{Flex, Label, CrossAxisAlignment};
use druid::{AppLauncher, Data, Env, Lens, LocalizedString,
            Widget, WidgetExt, WindowDesc};
use druid::im::{vector, Vector};

const WINDOW_TITLE: LocalizedString<TableState> = LocalizedString::new("Hello Table!");

#[derive(Clone, Data, Lens, Debug)]
struct HelloRow{
    lang: String,
    hello: String
}

impl HelloRow{
    fn new(lang: impl Into<String>, hello: impl Into<String> ) -> HelloRow{
        HelloRow{
            lang: lang.into(),
            hello: hello.into()
        }
    }
}

#[derive(Clone, Data, Lens)]
struct TableState {
    items: Vector<HelloRow>,
}

pub fn main() {
    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title(WINDOW_TITLE)
        .window_size((400.0, 400.0));

    // create the initial app state
    let initial_state = TableState {
        items: vector![
            HelloRow::new ("English", "Hello"),
            HelloRow::new("Français", "Bonjour"),
            HelloRow::new("Espanol", "Hola")
        ],
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}

fn build_root_widget() -> impl Widget<TableState> {
    // a label that will determine its text based on the current app data.
    let label = Label::new(|items: &Vector<HelloRow>, _env: &Env| format!("Table data: {:?}", items))
        .lens(TableState::items);


    let table = Table::<HelloRow>::new()
        .add_column("Language", TextCell::new().lens(HelloRow::lang))
        .add_column("Hello", TextCell::new().lens(HelloRow::hello))
        .lens(TableState::items);

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start )
        .with_child(label)
        .with_spacer(20.0)
        .with_child(table)
        .center()
}


