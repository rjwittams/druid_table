use std::fmt::Debug;

use druid_table::{
    column, CellRender, CellRenderExt, DataCompare, DefaultTableArgs, LogIdx, ShowHeadings,
    SortDirection, Table, TableAxis, TableBuilder, TextCell,
};

use druid::im::{vector, Vector};
use druid::kurbo::CircleSegment;
use druid::widget::{Button, CrossAxisAlignment, Flex, Label, RadioGroup, Split, ViewSwitcher};
use druid::{
    AppLauncher, Data, Env, KeyOrValue, Lens, LocalizedString, PaintCtx, Point, RenderContext,
    Widget, WidgetExt, WindowDesc,
};
use druid::{Color, Value};
use std::cmp::Ordering;
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
    show_headings: ShowHeadings,
}

struct PieCell {}

impl DataCompare<f64> for PieCell {
    fn compare(&self, a: &f64, b: &f64) -> Ordering {
        f64::partial_cmp(a, b).unwrap_or(Ordering::Equal)
    }
}

impl CellRender<f64> for PieCell {
    fn init(&mut self, _ctx: &mut PaintCtx, _env: &Env) {}

    fn paint(
        &self,
        ctx: &mut PaintCtx,
        _row_idx: LogIdx,
        _col_idx: LogIdx,
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

fn build_main_widget() -> impl Widget<TableState> {
    // Need a wrapper widget to get selection/scroll events out of it
    let row =|| HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63.);

    let buttons = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Modify table"))
        .with_child(
            Flex::column()
                .with_child(
                    Button::new("Add row")
                        .on_click(move |_, data: &mut Vector<HelloRow>, _| {
                            data.push_back(row());
                        })
                        .expand_width(),
                )
                .with_child(
                    Button::new("Remove row")
                        .on_click(|_, data: &mut Vector<HelloRow>, _| {
                            data.pop_back();
                        })
                        .expand_width(),
                ),
        )
        .lens(TableState::items);
    let headings_control = Flex::column()
        .with_child(Label::new("Headings to show:"))
        .with_child(RadioGroup::new(vec![
            ("Just cells", ShowHeadings::JustCells),
            ("Column headings", ShowHeadings::One(TableAxis::Columns)),
            ("Row headings", ShowHeadings::One(TableAxis::Rows)),
            ("Both", ShowHeadings::Both),
        ]))
        .lens(TableState::show_headings);
    let sidebar = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(buttons)
        .with_child(headings_control)
        .align_left();

    let vs = ViewSwitcher::new(
        |ts: &TableState, _| ts.show_headings.clone(),
        |sh, _, _| Box::new(build_table(sh.clone()).lens(TableState::items)),
    );

    Split::columns(vs, sidebar)
        .split_point(0.8)
        .draggable(true)
        .min_size(180.0)
}

fn build_table(show_headings: ShowHeadings) -> Table<DefaultTableArgs<Vector<HelloRow>>> {
    log::info!("Create table {:?}", show_headings);
    let table_builder = TableBuilder::<HelloRow, Vector<HelloRow>>::new()
        .headings(show_headings)
        .with_column("Language", TextCell::new().lens(HelloRow::lang))
        .with_column(
            "Greeting",
            TextCell::new().font_size(17.).lens(HelloRow::greeting),
        )
        .with_column(
            "Westernised",
            TextCell::new().font_size(17.).lens(HelloRow::westernised),
        )
        .with(
            column("Who knows?", PieCell {}.lens(HelloRow::who_knows))
                .sort(SortDirection::Ascending),
        )
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

    let table = Table::new(table_builder.build_args());

    table
}

pub fn main() {
    simple_logger::init().unwrap();

    info!("Hello table");

    // describe the main window
    let main_window = WindowDesc::new(build_main_widget)
        .title(WINDOW_TITLE)
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = TableState {
        items: vector![
            HelloRow::new("English", "Hello", "Hello", 99.1),
            HelloRow::new("Français", "Bonjour", "Bonjour", 95.0),
            HelloRow::new("Espanol", "Hola", "Hola", 95.0),
            HelloRow::new("Mandarin", "你好", "nǐ hǎo", 85.),
            HelloRow::new("Hindi", "नमस्ते", "namaste", 74.),
            HelloRow::new("Arabic", "مرحبا", "marhabaan", 24.),
            HelloRow::new("Portuguese", "olá", "olá", 30.),
            HelloRow::new("Russian", "Привет", "Privet", 42.),
            HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63.),
        ],
        show_headings: ShowHeadings::Both,
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
