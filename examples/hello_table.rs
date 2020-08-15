use std::fmt::Debug;

use druid_table::{
    column, AxisMeasurementType, CellCtx, CellRender, CellRenderExt, DataCompare,
    EditorFactory, ShowHeadings, SortDirection, Table, TableAxis, TableBuilder, TextCell,
};

use druid::im::{vector, Vector};
use druid::kurbo::CircleSegment;
use druid::theme::PLACEHOLDER_COLOR;
use druid::widget::{
    Button, Checkbox, CrossAxisAlignment, Flex, Label, MainAxisAlignment, Padding, RadioGroup,
    SizedBox, Stepper, ViewSwitcher,
};
use druid::{
    AppLauncher, Data, Env, KeyOrValue, Lens, LensExt, LocalizedString, PaintCtx, Point,
    RenderContext, Widget, WidgetExt, WindowDesc,
};
use druid::{Color, Value};
use std::cmp::Ordering;
use std::f64::consts::PI;

const WINDOW_TITLE: LocalizedString<HelloState> = LocalizedString::new("Hello Table!");

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

#[derive(Clone, Lens, Data, Debug)]
struct Settings {
    show_headings: ShowHeadings,
    border_thickness: f64,
    row_fixed: bool,
    col_fixed: bool,
}

impl PartialEq for Settings {
    fn eq(&self, other: &Self) -> bool {
        self.same(other)
    }
}

#[derive(Clone, Data, Lens)]
struct HelloState {
    items: Vector<HelloRow>,
    settings: Settings,
}

struct PieCell {}

impl DataCompare<f64> for PieCell {
    fn compare(&self, a: &f64, b: &f64) -> Ordering {
        f64::partial_cmp(a, b).unwrap_or(Ordering::Equal)
    }
}

impl CellRender<f64> for PieCell {
    fn init(&mut self, _ctx: &mut PaintCtx, _env: &Env) {}

    fn paint(&self, ctx: &mut PaintCtx, _cell: &CellCtx, data: &f64, _env: &Env) {
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

impl EditorFactory<f64> for PieCell {
    fn make_editor(&mut self, _ctx: &CellCtx) -> Option<Box<dyn Widget<f64>>> {
        None
    }
}

fn build_main_widget() -> impl Widget<HelloState> {
    // Need a wrapper widget to get selection/scroll events out of it
    let row = || HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63.);

    let buttons = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(decor(Label::new("Modify table")))
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
                )
                .padding(5.0),
        )
        .fix_width(200.0)
        .lens(HelloState::items);
    let headings_control = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(decor(Label::new("Headings to show")))
        .with_child(RadioGroup::new(vec![
            ("Just cells", ShowHeadings::JustCells),
            ("Column headings", ShowHeadings::One(TableAxis::Columns)),
            ("Row headings", ShowHeadings::One(TableAxis::Rows)),
            ("Both", ShowHeadings::Both),
        ]))
        .lens(HelloState::settings.then(Settings::show_headings));
    let style = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(decor(Label::new("Style")))
        .with_child(
            Flex::row()
                .with_child(Label::new("Border thickness"))
                .with_flex_spacer(1.0)
                .with_child(Label::new(|p: &f64, _: &Env| p.to_string()))
                .with_child(Stepper::new().with_range(0., 20.0).with_step(0.5))
                .lens(HelloState::settings.then(Settings::border_thickness)),
        );

    let measurements = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(decor(Label::new("Uniform axes")))
        .with_child(Flex::row().with_child(Checkbox::new("Rows").lens(Settings::row_fixed)))
        .with_child(Flex::row().with_child(Checkbox::new("Columns").lens(Settings::col_fixed)))
        .lens(HelloState::settings);

    let sidebar = Flex::column()
        .main_axis_alignment(MainAxisAlignment::Start)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(group(buttons))
        .with_child(group(headings_control))
        .with_child(group(style))
        .with_child(group(measurements))
        .with_flex_spacer(1.)
        .fix_width(200.0);

    let vs = ViewSwitcher::new(
        |ts: &HelloState, _| ts.settings.clone(),
        |sh, _, _| Box::new(build_table(sh.clone()).lens(HelloState::items)),
    )
    .padding(10.);

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(sidebar)
        .with_flex_child(vs, 1.)
}

fn decor<T: Data>(label: Label<T>) -> SizedBox<T> {
    label
        .padding(5.)
        .background(PLACEHOLDER_COLOR)
        .expand_width()
}

fn group<T: Data, W: Widget<T> + 'static>(w: W) -> Padding<T> {
    w.border(Color::WHITE, 0.5).padding(5.)
}

fn build_table(settings: Settings) -> impl Widget<Vector<HelloRow>> {
    let table_builder = TableBuilder::<HelloRow, Vector<HelloRow>>::new()
        .measuring_axis(
            &TableAxis::Rows,
            if settings.row_fixed {
                AxisMeasurementType::Uniform
            } else {
                AxisMeasurementType::Individual
            },
        )
        .measuring_axis(
            &TableAxis::Columns,
            if settings.col_fixed {
                AxisMeasurementType::Uniform
            } else {
                AxisMeasurementType::Individual
            },
        )
        .headings(settings.show_headings)
        .border(settings.border_thickness)
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

    let measures = table_builder.build_measures();
    let table = Table::new_in_scope(table_builder.build_args(), measures);

    table
}

pub fn main() {
    simple_logger::init().unwrap();

    // describe the main window
    let main_window = WindowDesc::new(build_main_widget)
        .title(WINDOW_TITLE)
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = HelloState {
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
        settings: Settings {
            show_headings: ShowHeadings::Both,
            border_thickness: 1.,
            row_fixed: false,
            col_fixed: false,
        },
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}
