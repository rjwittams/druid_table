use druid::kurbo::{Line, Point, Rect, Size};
use druid::widget::{Axis, CrossAxisAlignment};
use druid::{AppLauncher, Color, Data, Lens, Widget, WindowDesc};
use druid_table::{BandScale, DrawableAxis, F64Range, LinearScale, LogIdx, Mark, MarkId, MarkShape, Vis, VisPolicy, VisEvent};
use im::Vector;
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::hash::Hash;

#[macro_use]
extern crate im;

// Working from
// https://vega.github.io/vega/examples/bar-chart/

fn main_widget()->impl Widget<TopLevel>{
    Vis::new(MyBarChart{
        tooltip_item: None
    } )
}

fn main() {
    let main_window = WindowDesc::new(main_widget)
        .title("Visualisation")
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = TopLevel {
        records: vector![
            ("A".into(), 28),
            ("B".into(), 55),
            ("C".into(), 43),
            ("D".into(), 91),
            ("E".into(), 81),
            ("F".into(), 53),
            ("G".into(), 19),
            ("H".into(), 87)
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
}

struct MyBarChart{
    tooltip_item: Option<CatCount>
}

impl VisPolicy for MyBarChart {
    type Input = TopLevel;
    type Scales = (BandScale<String>, LinearScale<u32>);

    fn event(&mut self, data: &mut Self::Input, event_marks: &mut Vec<Mark>, scales: &Self::Scales, event: &VisEvent) -> bool {
        log::info!("Event {:?}", event);
        let chg = match event {
            VisEvent::MouseEnter(MarkId::Datum { idx })=>{
                self.tooltip_item = data.records.get(idx.0).cloned();
                true
            },
            VisEvent::MouseOut(_)=>{
                self.tooltip_item = None;
                true
            }
            _=>false
        };
        if chg {
            if let Some(tt) = &self.tooltip_item {
                let (x, y) = scales;
                event_marks.clear();
                event_marks.push(Mark::new(MarkId::Unknown, MarkShape::Text {
                    txt: tt.1.to_string(),
                    font_fam: Default::default(),
                    size: 12.0,
                    point: Point::new(
                        x.range_val( &tt.0).mid(),
                        y.range_val( &tt.1) - 2.0
                    )
                }, Color::rgb8(0xD0, 0xD0, 0xD0), None));
            }
        }
        chg
    }

    fn scales(&self, data: &Self::Input, size: Size) -> Self::Scales {
        (
            BandScale::new(
                F64Range(30.0, size.width),
                &mut data.records.iter().map(|x| (x.0).clone()),
                0.05,
            ),
            LinearScale::new(
                F64Range(30.0, size.height - 10.0),
                &mut data.records.iter().map(|x| (x.1).clone()),
                true,
                None,
                true,
            ),
        )
    }

    fn data_marks(&self, data: &Self::Input, scales: &Self::Scales) -> Vec<Mark> {
        let (x, y) = scales;
        data.records
            .iter()
            .enumerate()
            .map(|(idx, (cat, amount))| {
                let xr = x.range_val(cat);
                let r = Rect::new(xr.0, y.range.0, xr.1, y.range_val(amount));
                Mark::new(
                    MarkId::Datum { idx: LogIdx(idx) },
                    MarkShape::Rect(r),
                    Color::rgb8(0x46, 0x82, 0xb4),
                    Some(Color::rgb8(0xFF, 0, 0)),
                )
            })
            .collect()
    }

    fn drawable_axes(&self, scales: &Self::Scales) -> Vec<DrawableAxis> {
        let (x, y) = scales;
        vec![x.make_axis(), y.make_axis()]
    }
}
