use druid::kurbo::{Point, Rect, Size};
use druid::widget::{Button, Flex};
use druid::{AppLauncher, Color, Data, Lens, Widget, WindowDesc};
use druid_table::{
    AxisName, BandScale, BandScaleFactory, DatumId, DrawableAxis, F64Range, LinearScale, LogIdx,
    Mark, MarkId, MarkShape, OffsetSource, SeriesId, StateName, TextMark, Vis, VisEvent,
    Visualization,
};
use im::Vector;
use std::collections::{HashMap};

#[macro_use]
extern crate im;
use rand::rngs::ThreadRng;
use rand::Rng;

// Working from
// https://vega.github.io/vega/examples/bar-chart/

fn main_widget() -> impl Widget<TopLevel> {
    Flex::column()
        .with_child(
            Button::new("Change data").on_click(|_, tl: &mut TopLevel, _| {
                let mut rng = rand::thread_rng();
                for (_, am) in tl.records.iter_mut().skip(1) {
                    if rng.gen_bool(0.3) {
                        *am = (*am as i32 + (rng.gen_range(-0.15, 0.16) * *am as f64) as i32).max(0)
                            as u32;
                    }
                }

                let move_rand =
                    |rng: &mut ThreadRng, a: &mut Vector<CatCount>, b: &mut Vector<CatCount>| {
                        if !a.is_empty() {
                            let idx = if a.len() == 1 {
                                0
                            } else {
                                rng.gen_range(0, a.len() - 1)
                            };
                            let rem = a.remove(idx);
                            b.push_back(rem)
                        }
                    };

                if rng.gen_bool(0.1) {
                    move_rand(&mut rng, &mut tl.records, &mut tl.others);
                }

                if rng.gen_bool(0.1) {
                    move_rand(&mut rng, &mut tl.others, &mut tl.records);
                }
            }),
        )
        .with_flex_child(Vis::new(MyBarChart::new()), 1.)
}

fn main() {
    let main_window = WindowDesc::new(main_widget)
        .title("Visualisation")
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = TopLevel {
        records: vector![
            ("A".into(), 284),
            ("B".into(), 554),
            ("C".into(), 433),
            ("D".into(), 912),
            ("E".into(), 814),
            ("F".into(), 533),
            ("G".into(), 870),
            ("H".into(), 872)
        ],
        others: vector![
            ("I".into(), 342),
            ("J".into(), 294),
            ("K".into(), 766),
            ("L".into(), 996)
        ],
    };

    // start the application
    AppLauncher::with_window(main_window)
        .use_simple_logger()
        .launch(initial_state)
        .expect("Failed to launch application");
}

type CatCount = (String, u32);

#[derive(Clone, Data, Lens)]
struct TopLevel {
    records: Vector<CatCount>,
    others: Vector<CatCount>,
}

struct MyBarChart {
    x: BandScaleFactory<String>,
    record_offsets: OffsetSource<String, LogIdx>,
    rec_to_idx: HashMap<LogIdx, usize>,
}

impl MyBarChart {
    pub fn new() -> Self {
        MyBarChart {
            x: BandScaleFactory::new(AxisName("x")),
            record_offsets: Default::default(),
            rec_to_idx: Default::default(),
        }
    }
}

impl Visualization for MyBarChart {
    type Input = TopLevel;
    type State = Option<CatCount>;
    type Layout = (BandScale<String>, LinearScale<u32>);

    fn layout(&mut self, data: &Self::Input, size: Size) -> Self::Layout {
        (
            self.x.make_scale(
                F64Range(30.0, size.width),
                &mut data.records.iter().map(|x| (x.0).clone()),
                0.05,
            ),
            LinearScale::new(
                AxisName("y"),
                F64Range(30.0, size.height - 10.0),
                &mut data.records.iter().map(|x| (x.1).clone()),
                true,
                None,
                true,
            ),
        )
    }

    fn event(
        &self,
        data: &mut Self::Input,
        _layout: &Self::Layout,
        tooltip_item: &mut Option<CatCount>,
        event: &VisEvent,
    ) {
        match event {
            VisEvent::MouseEnter(MarkId::Datum(DatumId {
                series: SeriesId(0),
                idx,
            })) => {
                *tooltip_item = self
                    .rec_to_idx
                    .get(idx)
                    .and_then(|idx| data.records.get(*idx).cloned())
            }
            VisEvent::MouseOut(_) => *tooltip_item = None,
            e => log::info!("Did not match event {:?}", e),
        };
    }

    fn layout_marks(&self, (x, y): &Self::Layout) -> Vec<DrawableAxis> {
        vec![x.make_axis(), y.make_axis()]
    }

    fn state_marks(
        &self,
        _data: &Self::Input,
        (x, y): &Self::Layout,
        tooltip_item: &Option<CatCount>,
    ) -> Vec<Mark> {
        let mut marks = Vec::new();
        if let Some(tt) = tooltip_item {
            marks.push(Mark::new(
                MarkId::StateMark(StateName("tooltip"), 0),
                MarkShape::Text(TextMark::new(
                    tt.1.to_string(),
                    Default::default(),
                    12.0,
                    Point::new(x.range_val(&tt.0).mid(), y.range_val(&tt.1) - 2.0),
                )),
                Color::rgb8(0xD0, 0xD0, 0xD0),
                None,
            ));
        }
        marks
    }

    fn data_marks(&mut self, data: &Self::Input, (x, y): &Self::Layout) -> Vec<Mark> {
        self.rec_to_idx.clear();

        data.records
            .iter()
            .enumerate()
            .map(|(idx, (cat, amount))| {
                let xr = x.range_val(cat);
                let r = Rect::new(xr.0, y.range.0, xr.1, y.range_val(amount));
                let record_offset = self.record_offsets.offset(cat);
                self.rec_to_idx.insert(record_offset, idx);
                Mark::new(
                    MarkId::Datum(DatumId::new(SeriesId(0), record_offset)),
                    MarkShape::Rect(r),
                    Color::rgb8(0x46, 0x82, 0xb4),
                    Some(Color::rgb8(0xFF, 0, 0)),
                )
            })
            .collect()
    }
}
