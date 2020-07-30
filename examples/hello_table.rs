use std::fmt::Debug;

use druid_table::{CellRender, CellRenderExt, TableBuilder, TextCell};

use druid::im::{vector, Vector};
use druid::kurbo::CircleSegment;
use druid::{AppLauncher, Data, Env, KeyOrValue, Lens, LocalizedString, PaintCtx, Point, RenderContext, Widget, WidgetExt, WindowDesc};
use druid::{Color, Value};
use std::f64::consts::PI;

#[macro_use]
extern crate log;

const WINDOW_TITLE: LocalizedString<TableState> = LocalizedString::new("Hello Table!");

#[derive(Clone, Data, Lens, Debug)]
struct HelloRow {
    lang: String,
    greeting: String,
    westernised: String,
    who_knows: f64,
}

impl HelloRow {
    fn new(
        lang: impl Into<String>,
        greeting: impl Into<String>,
        westernised: impl Into<String>,
        percent: f64,
    ) -> HelloRow {
        HelloRow {
            lang: lang.into(),
            greeting: greeting.into(),
            westernised: westernised.into(),
            who_knows: percent / 100.,
        }
    }
}

#[derive(Clone, Data, Lens)]
struct TableState {
    items: Vector<HelloRow>,
}

struct PieCell {}

impl CellRender<f64> for PieCell {
    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        _row_idx: usize,
        _col_idx: usize,
        data: &f64,
        _env: &Env,
    ) {
        let rect = ctx.region().to_rect().with_origin(Point::ORIGIN);

        //ctx.stroke( rect, &Color::rgb(0x60, 0x0, 0x10), 2.);
        let circle = CircleSegment::new(
            rect.center(),
            (f64::min(rect.height(), rect.width()) / 2.) - 2.,
            0.,
            0.,
            2. * PI * *data,
        );
        ctx.fill(&circle, &Color::rgb(0x0, 0xFF, 0x0));

        ctx.stroke(&circle, &Color::BLACK, 1.0);
    }
}

fn build_root_widget() -> impl Widget<TableState> {
    let table_builder = TableBuilder::<HelloRow, Vector<HelloRow>>::new()
        .with_column("Language", TextCell::new().lens(HelloRow::lang))
        .with_column(
            "Greeting",
            TextCell::new().font_size(17.).lens(HelloRow::greeting),
        )
        .with_column(
            "Westernised",
            TextCell::new().font_size(17.).lens(HelloRow::westernised),
        )
        .with_column("Who knows?", PieCell {}.lens(HelloRow::who_knows))
        .with_column(
            "Greeting 2 with very long column name",
            TextCell::new()
                .font_name(KeyOrValue::Concrete(Value::String("Courier New".into())))
                .lens(HelloRow::greeting),
        )
        .with_column(
            "Greeting 3",
            TextCell::new()
                .text_color(Color::rgb(0xD0, 0, 0))
                .lens(HelloRow::greeting),
        )
        .with_column("Greeting 4", TextCell::new().lens(HelloRow::greeting))
        .with_column("Greeting 5", TextCell::new().lens(HelloRow::greeting))
        .with_column("Greeting 6", TextCell::new().lens(HelloRow::greeting));

    table_builder.build_widget().lens(TableState::items)
}

pub fn main() {
    simple_logger::init().unwrap();

    info!("Hello table");

    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title(WINDOW_TITLE)
        .window_size((400.0, 300.0));

    // create the initial app state
    let initial_state = TableState {
        items: vector![
            HelloRow::new("English", "Hello", "Hello", 99.1),
            HelloRow::new("Français", "Bonjour", "Bonjour", 91.9),
            HelloRow::new("Espanol", "Hola", "Hola", 95.0),
            HelloRow::new("Mandarin", "你好", "nǐ hǎo", 85.),
            HelloRow::new("Hindi", "नमस्ते", "namaste", 74.),
            HelloRow::new("Arabic", "مرحبا", "marhabaan", 24.),
            HelloRow::new("Portuguese", "olá", "olá", 30.),
            HelloRow::new("Russian", "Привет", "Privet", 42.),
            HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63.),
        ],
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}