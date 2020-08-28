use druid::{Widget, LifeCycle, EventCtx, PaintCtx, LifeCycleCtx, BoxConstraints, LayoutCtx, Event, Env, UpdateCtx, Color, Data};
use druid::kurbo::{Size, Rect, Affine, Vec2, Line, Point, ParamCurveDeriv, ParamCurveCurvature, ParamCurveNearest};
use druid::piet::{FontFamily, Text, TextLayoutBuilder, TextLayout};
use druid::widget::prelude::RenderContext;
use druid::widget::{Axis, CrossAxisAlignment};
use std::hash::Hash;
use std::fmt::Display;
use std::collections::{HashMap, BTreeSet};
use itertools::Itertools;
use itertools::__std_iter::{Chain, FlatMap};
use std::slice::Iter;
use std::ops::{Add, Sub, Mul};
use std::f64::NAN;
use std::marker::PhantomData;
use std::f64::consts::LN_10;
use crate::LogIdx;

#[derive(Debug)]
pub struct Mark{
    id: MarkId,
    shape: MarkShape,
    color: Color,
    hover: Option<Color>
}

impl Mark {
    pub fn new(id: MarkId, shape: MarkShape, color: Color, hover: Option<Color>) -> Self {
        Mark { id,  shape, color, hover}
    }

    pub fn hit(&self, pos: Point)->bool{
        match self.shape{
            MarkShape::Rect(r)=>r.contains(pos),
            MarkShape::Line(l)=>{
                let (_, d2) = l.nearest(pos, 1.0);
                d2 < 1.0
            }
            _=>false
        }
    }
}

pub enum MarkText{

}

#[derive(Debug)]
pub enum MarkShape{
    Rect(Rect),
    Line(Line),
    Text{txt:String, font_fam:FontFamily, size:f64, point: Point},
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum MarkId{
    Datum{idx:LogIdx}, // Series, Generator etc
    Unknown
}

pub struct DrawableAxis{
    axis: Axis,
    cross_axis_alignment: CrossAxisAlignment,
    marks: Vec<Mark>,
    size: Size
}

impl DrawableAxis {
    pub fn new(axis: Axis, cross_axis_alignment: CrossAxisAlignment, marks: Vec<Mark>, size: Size) -> Self {
        DrawableAxis { axis, cross_axis_alignment, marks, size }
    }
}

#[derive(Debug)]
pub enum VisEvent{
    MouseEnter(MarkId),
    MouseOut(MarkId)
}

pub trait VisPolicy{
    type Input : Data;
    type Scales;

    fn event(&mut self, data: &mut Self::Input, event_marks:&mut Vec<Mark>, scales: &Self::Scales, event: &VisEvent)->bool;
    fn scales(&self, data: &Self::Input, size: Size)->Self::Scales;
    fn data_marks(&self, data: &Self::Input, scales: &Self::Scales) ->Vec<Mark>;
    fn drawable_axes(&self, scales: &Self::Scales) ->Vec<DrawableAxis>;
}

struct VisState<VP: VisPolicy>{
    scales: VP::Scales,
    event_marks: Vec<Mark>,
    data_marks: Vec<Mark>,
    drawable_axes: Vec<DrawableAxis>,
    transform: Affine,
    focus: Option<MarkId>
}

impl<VP: VisPolicy> VisState<VP> {
    pub fn new(scales: VP::Scales, data_marks: Vec<Mark>, drawable_axes: Vec<DrawableAxis>, transform: Affine) -> Self {
        VisState { scales, event_marks: vec![], data_marks, drawable_axes, transform, focus: None }
    }

    fn find_mark(&mut self, pos: Point)->Option<&mut Mark>{
        self.data_marks.iter_mut().filter(|mark| mark.hit(pos)).next()
    }
}

pub struct Vis<P: VisPolicy>{
    policy: P,
    state: Option<VisState<P>>
}

impl<P: VisPolicy> Vis<P> {
    pub fn new(policy: P) -> Self {
        Vis { policy, state: None }
    }

    fn paint_marks<'a>(ctx: &mut PaintCtx, focus: &Option<MarkId>, marks: &mut impl Iterator<Item=&'a Mark>) {
        for mark in marks {
            let color = match (mark.id, focus){
                (id, Some(f)) if id == *f =>mark.hover.as_ref().unwrap_or( &mark.color ),
                _=>&mark.color
            };
            match &mark.shape {
                MarkShape::Rect(r) => {
                    ctx.stroke(r, &Color::BLACK, 1.0);

                    ctx.fill(r, color);
                },
                MarkShape::Line(l) => {
                    ctx.stroke(l, color, 1.0);
                },
                MarkShape::Text { txt, font_fam, size, point } => {
                    // TODO: Put the layout in the text?
                    let tl = ctx.text().new_text_layout(&txt).font(font_fam.clone(), *size).text_color(color.clone()).build().unwrap();
                    ctx.with_save( |ctx| {
                        // Flip the coordinates back to draw text
                        ctx.transform( Affine::translate( point.to_vec2() - Vec2::new( tl.size().width / 2., 0.) ) * Affine::FLIP_Y );
                        ctx.draw_text(&tl, Point::ORIGIN);
                    });
                }
            }
        }
    }

    fn ensure_state(&mut self, data: &P::Input, sz: Size) -> &mut VisState<P> {
        if self.state.is_none() {
            let scales = self.policy.scales(data, sz);
            let marks = self.policy.data_marks(data, &scales);
            let axes = self.policy.drawable_axes( &scales);
            self.state = Some(VisState::new(scales, marks, axes,  Affine::FLIP_Y * Affine::translate(Vec2::new(0., -sz.height))));
        }
        self.state.as_mut().unwrap()
    }
}

impl <VP: VisPolicy> Widget<VP::Input> for Vis<VP>{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut VP::Input, env: &Env) {
        self.ensure_state(data, ctx.size());
        let state = self.state.as_mut().unwrap();

        match event {
            Event::MouseMove(me) => {
                if let Some(mark) = state.find_mark( state.transform.inverse() * me.pos ){

                    match mark.id {
                        MarkId::Unknown => {}
                        _ => {
                            let mi = mark.id.clone();
                            if state.focus != Some(mi) {
                                self.policy.event(data, &mut state.event_marks,  &state.scales, &VisEvent::MouseEnter(mi));
                                state.focus = Some(mi);
                                ctx.request_paint();
                            }
                        }
                    }
                }else{
                    if let Some(focus) = state.focus {
                        let scales = &state.scales;
                        if self.policy.event(data, &mut state.event_marks, scales,&VisEvent::MouseOut(focus)){
                            ctx.request_paint()
                        }
                    }
                    if state.focus.is_some(){
                        state.focus = None;
                        ctx.request_paint()
                    }
                }
            }
            _=>{}
        }
        ()
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &VP::Input, env: &Env) {
        if let LifeCycle::WidgetAdded = event{

        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &VP::Input, data: &VP::Input, env: &Env) {
        if !data.same(old_data){
            self.state = None;
            ctx.request_paint()
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &VP::Input, env: &Env) -> Size {
        self.state = None;
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &VP::Input, env: &Env) {
        let size = ctx.size();

        let state = self.ensure_state(data, size);

        ctx.with_save(|ctx| {
            ctx.transform(state.transform);

            Self::paint_marks(ctx, &state.focus, &mut state.data_marks.iter());

            for axis in state.drawable_axes.iter() {
                Self::paint_marks(ctx, &state.focus, &mut axis.marks.iter());
            }

            Self::paint_marks(ctx, &state.focus, &mut state.event_marks.iter());
        });
    }
}

pub struct BandScale<T: Clone + Ord + Hash + Display>{
    range: F64Range,
    bands: Vec<T>,
    bands_lookup: HashMap<T, usize>,
    range_per_band: f64,
    half_padding: f64
}

impl <T: Clone + Ord + Hash + Display> BandScale<T> {
    pub fn new(range: F64Range, bands_it: &mut impl Iterator<Item=T>, padding_ratio: f64) -> Self {
        let mut uniq = BTreeSet::new();
        for item in bands_it{
            uniq.insert(item);
        }
        let bands: Vec<T> = uniq.iter().cloned().collect();
        let bands_lookup: HashMap<T, usize> = uniq.into_iter().enumerate().map(|(i, v)|(v, i)).collect();
        let range_per_band = range.distance() / (bands.len() as f64);
        let half_padding = padding_ratio * range_per_band / 2.;
        BandScale {
            range,
            bands,
            bands_lookup,
            range_per_band,
            half_padding
        }
    }

    pub fn range_val(&self, domain_val: &T) -> F64Range{
        let idx = self.bands_lookup.get(domain_val).unwrap();
        let start = self.range.0 + ((*idx as f64) * self.range_per_band);
        F64Range(start + self.half_padding, start + self.range_per_band - self.half_padding)
    }

    pub fn make_axis(&self)->DrawableAxis{
        let mut marks = Vec::new();
        let line_y = 30.0;
        let tick_extent = line_y - 8.;
        let label_top = tick_extent - 2.;
        marks.push( Mark::new(MarkId::Unknown, MarkShape::Line(Line::new((self.range.0, line_y), (self.range.1, line_y))), Color::WHITE, None));
        for v in self.bands.iter(){
            let b_mid = self.range_val(v).mid();
            marks.push(Mark::new(MarkId::Unknown,MarkShape::Line(Line::new((b_mid, tick_extent), (b_mid, line_y))), Color::WHITE, None));
            marks.push( Mark::new(MarkId::Unknown,MarkShape::Text{
                txt: v.to_string(),
                font_fam: Default::default(),
                size: 12.0,
                point: Point::new( b_mid, label_top )
            }, Color::WHITE, None) );
        }
        DrawableAxis::new(Axis::Horizontal, CrossAxisAlignment::Start, marks, Size::new(self.range.1 - self.range.0, line_y) )
    }
}

pub trait LinearValue: Clone + Ord + Display{
    fn as_f64(&self)->f64;
    fn from_f64(val: f64)->Self;
}

impl LinearValue for u32{
    fn as_f64(&self) -> f64 {
        *self as f64
    }

    fn from_f64(val: f64) -> Self {
        val as u32
    }
}

#[derive(Copy, Clone, Debug)]
pub struct F64Range(pub f64, pub f64);

lazy_static! {
static ref e10: f64 = 50.0_f64.sqrt();
static ref e5: f64 = 10.0_f64.sqrt();
static ref e2: f64 = 2.0_f64.sqrt();
}

impl F64Range {
    pub fn distance(self)->f64{
        self.1 - self.0
    }

    pub fn mid(self) ->f64{
        (self.0 + self.1) / 2.
    }

    fn step_size(self, count: usize) ->f64{
            let step = self.distance() / count.max(0) as f64 ;
            let power = ( step.ln() / LN_10 ).floor();
            let error = step / 10.0_f64.powf(power);

         let factor = if error >= *e10 {
             10.
         }else if error >= *e5 {
             5.
         }else if error >= *e2 {
             2.
         }else {
             1.
         };

         if power >= 0. {
              factor * 10.0_f64.powf(power)
         }else {
             -(10.0_f64.powf(-power)) / factor
         }
    }

    // Fairly close to what D3 does here https://github.com/d3/d3-scale/blob/master/src/linear.js
    // TODO: decrementing ranges
    fn nice(self, count: usize) -> (F64Range, f64){
        let max_iter = 10;
        let mut pre_step: f64 = NAN;
        let mut current = self;

        for _ in 0..max_iter{
            let step = current.step_size(count);
            if step == pre_step{
                break;
            }else {
                let F64Range(start, stop) = current;
                current = F64Range(
                    (start / step).floor()  * step,
                    (stop / step).ceil() * step
                );
            }
            pre_step = step;
        }
        (current, pre_step)
    }

    fn include_zero(self, inc: bool)->Self{
        if inc{
            // TODO: negative / flipped
            F64Range(self.0.min(0.), self.1.max(0.) )
        }else{
            self
        }
    }
}

pub struct LinearScale<T: LinearValue>{
    pub range: F64Range,
    domain_range: F64Range,
    multiplier: f64,
    ticks: usize,
    tick_step: f64,
    phantom_t: PhantomData<T>
}

impl <T: LinearValue > LinearScale<T> {
    pub fn new(range: F64Range, domain_iter: &mut impl Iterator<Item=T>, nice: bool, ticks_goal: Option<usize>, zero: bool) -> Self {
        let ticks = ticks_goal.unwrap_or(10);
        let (start, stop) = domain_iter.minmax().into_option().unwrap();
        let domain_range = F64Range(start.as_f64(), stop.as_f64()).include_zero(zero);
        let (domain_range, tick_step) = if nice{ domain_range.nice(ticks) } else { (domain_range, domain_range.step_size(ticks)) };
        let domain_dist = domain_range.distance();

        let ticks = (domain_dist / tick_step).ceil() as usize;
        let multiplier = range.distance() / domain_dist;
        LinearScale {range, domain_range, multiplier, ticks, tick_step, phantom_t: Default::default()}
    }

    pub fn range_val(&self, domain_val: &T) -> f64{
        self.range_val_raw(domain_val.as_f64())
    }

    pub fn range_val_raw(&self, domain_float: f64) -> f64{
        self.range.0 + self.multiplier * ( domain_float - self.domain_range.0 )
    }

    pub fn make_axis(&self)->DrawableAxis{
        let mut marks = Vec::new();
        let line_x = 30.0;
        let tick_extent = line_x - 8.;
        let label_x = tick_extent - 2.;
        marks.push( Mark::new(MarkId::Unknown,MarkShape::Line(Line::new((line_x, self.range.0), (line_x, self.range.1))), Color::WHITE, None));


        for step in 0..=self.ticks {
            let d_v = self.domain_range.0 + self.tick_step * (step as f64);
            let value = T::from_f64(d_v);

            let r_v = self.range_val_raw(d_v);
            marks.push(Mark::new(MarkId::Unknown,MarkShape::Line(Line::new((tick_extent, r_v), (line_x, r_v))), Color::WHITE, None));
            marks.push( Mark::new(MarkId::Unknown,MarkShape::Text{
                txt: value.to_string(),
                font_fam: Default::default(),
                size: 12.0,
                point: Point::new( label_x - 5.0, r_v + 8.0)
            }, Color::WHITE, None));
        }
        DrawableAxis::new(Axis::Vertical, CrossAxisAlignment::Start, marks, Size::new(self.range.1 - self.range.0, line_x) )
    }
}
