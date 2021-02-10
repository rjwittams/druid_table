use std::fmt::Debug;

use druid_table::{
    column, AxisMeasurementType, CellCtx, CellDelegate, CellsDelegate, DataCompare, DisplayFactory,
    DisplayFactoryExt, ShowHeadings, SortDirection, Table, TableAxis, TableBuilder,
    WidgetCell,
};

use druid::im::{vector, Vector};
use druid::kurbo::CircleSegment;
use druid::theme::PLACEHOLDER_COLOR;
use druid::widget::{
    Button, Checkbox, CrossAxisAlignment, Flex, Label, MainAxisAlignment, Padding, Painter,
    RadioGroup, RawLabel, SizedBox, Stepper, TextBox, ViewSwitcher,
};
use druid::{
    AppLauncher, Data, Env, Event, EventCtx, FontDescriptor, FontFamily, KeyOrValue, Lens, LensExt,
    LocalizedString, PaintCtx, Point, RenderContext, Widget, WidgetExt, WindowDesc,
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
    complete: bool,
}

impl HelloRow {
    fn new(
        lang: impl Into<String>,
        greeting: impl Into<String>,
        westernised: impl Into<String>,
        percent: f64,
        complete: bool,
    ) -> HelloRow {
        HelloRow {
            lang: lang.into(),
            greeting: greeting.into(),
            westernised: westernised.into(),
            who_knows: percent / 100.,
            complete,
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

fn pie_cell<Row: Data, MakeLens: Fn() -> L, L: Lens<Row, f64> + 'static>(
    make_lens: MakeLens,
) -> impl CellDelegate<Row> {
    WidgetCell::new_unsorted(
        |cell| {
            Painter::new(|ctx: &mut PaintCtx, data: &f64, _env: &Env| {
                let rect = ctx.size().to_rect().inset(-5.);
                let circle = CircleSegment::new(
                    rect.center(),
                    (f64::min(rect.height(), rect.width()) / 2.),
                    0.,
                    0.,
                    2. * PI * *data,
                );
                ctx.fill(&circle, &Color::rgb8(0x0, 0xFF, 0x0));
                ctx.stroke(&circle, &Color::BLACK, 1.5);
            })
        },
        make_lens,
    )
    .edit_with(|cell| Stepper::new().with_range(0.0, 1.0).with_step(0.02))
    .compare_with(|a, b| f64::partial_cmp(a, b).unwrap_or(Ordering::Equal))
}

fn build_main_widget() -> impl Widget<HelloState> {
    // Need a wrapper widget to get selection/scroll events out of it
    let row = || HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63., true);

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

fn build_table(settings: Settings) -> Table<Vector<HelloRow>> {
    let measurement_type = if settings.col_fixed {
        AxisMeasurementType::Uniform
    } else {
        AxisMeasurementType::Individual
    };
    TableBuilder::<Vector<HelloRow>>::new()
        .measuring_axis(TableAxis::Rows, measurement_type)
        .measuring_axis(TableAxis::Columns, measurement_type)
        .headings(settings.show_headings)
        .border(settings.border_thickness)
        .with_column(
            "Language",
            WidgetCell::new(
                |cell| {
                    RawLabel::new()
                        .with_font(FontDescriptor::new(FontFamily::SERIF))
                        .with_text_size(15.)
                        .with_text_color(Color::BLUE)
                },
                || HelloRow::lang,
            )
            .compare_with(|a, b| a.len().cmp(&b.len()))
            .edit_with(|cell| TextBox::new()),
        )
        .with_column(
            "Complete",
            WidgetCell::new(|cell| Checkbox::new(""), || HelloRow::complete),
        )
        .with_column("Greeting", WidgetCell::text(|| HelloRow::greeting))
        .with_column(
            "Westernised",
            WidgetCell::text_configured(|rl| rl.with_text_size(17.), || HelloRow::westernised),
        )
        .with(column("Who knows?", pie_cell(|| HelloRow::who_knows)).sort(SortDirection::Ascending))
        .with_column(
            "Greeting 2 with very long column name",
            WidgetCell::text_configured(
                |rl| {
                    rl.with_font(FontDescriptor::new(FontFamily::new_unchecked(
                        "Courier New",
                    )))
                },
                || HelloRow::greeting,
            ),
        )
        .with_column(
            "Greeting 3",
            WidgetCell::text_configured(
                |rl| rl.with_text_color(Color::rgb8(0xD0, 0, 0)),
                || HelloRow::greeting,
            ),
        )
        .with_column("Greeting 4", WidgetCell::text(|| HelloRow::greeting))
        .with_column("Greeting 5", WidgetCell::text(|| HelloRow::greeting))
        .with_column("Greeting 6", WidgetCell::text(|| HelloRow::greeting))
        .build()
}

pub fn main() {
    // describe the main window
    let main_window = WindowDesc::new(build_main_widget())
        .title(WINDOW_TITLE)
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = HelloState {
        items: vector![
            HelloRow::new("English", "Hello", "Hello", 99.1, true),
            HelloRow::new("Français", "Bonjour", "Bonjour", 95.0, false),
            HelloRow::new("Espanol", "Hola", "Hola", 95.0, true),
            HelloRow::new("Mandarin", "你好", "nǐ hǎo", 85., false),
            HelloRow::new("Hindi", "नमस्ते", "namaste", 74., true),
            HelloRow::new("Arabic", "مرحبا", "marhabaan", 24., true),
            HelloRow::new("Portuguese", "olá", "olá", 30., false),
            HelloRow::new("Russian", "Привет", "Privet", 42., false),
            HelloRow::new("Japanese", "こんにちは", "Kon'nichiwa", 63., false),
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
        .use_simple_logger()
        .launch(initial_state)
        .expect("Failed to launch application");
}
