use crate::LogIdx;
use druid::kurbo::{Affine, Line, ParamCurveNearest, Point, Rect, Size, Vec2, BezPath};
use druid::piet::{FontFamily, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::RenderContext;
use druid::{BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, UpdateCtx, Widget, Value};
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::LN_10;
use std::f64::NAN;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::{Add};
use InterpError::*;
use std::fmt;
use crate::data::Remap::Selected;

#[derive(Debug, Copy, Clone)]
struct AnimationSegmentState {
    fraction: f64,
    eased_fraction: f64
}

impl AnimationSegmentState {
    const START : Self = Self::new(0.0, 0.0);
    const END : Self = Self::new(1.0, 1.0);

    pub const fn new(fraction: f64, eased_fraction: f64) -> Self {
        AnimationSegmentState { fraction, eased_fraction }
    }
}


struct AnimationCtx<'a> {
    current_segment: usize,
    segment_fractions: &'a [AnimationSegmentState],
}

impl AnimationCtx<'_> {
    pub fn new(current_segment: usize, segment_fractions: &[AnimationSegmentState]) -> AnimationCtx {
        if current_segment >= segment_fractions.len() {
            panic!("animation segment out of range {:?} {:?}", current_segment, segment_fractions)
        }
        AnimationCtx { current_segment, segment_fractions }
    }

    pub fn current(&self) -> f64 {
        self.segment_fractions[self.current_segment].eased_fraction
    }
}

#[derive(Debug, Data, Clone)]
pub struct Mark {
    id: MarkId,
    shape: MarkShape,
    color: Color,
    original: Color,
    hover: Option<Color>, // Maybe a bunch more properties / states, but could be dependent on state or something?
                          // Could be somewhere else perhaps
}

#[derive(Eq, PartialEq, Debug)]
enum InterpError {
    ValueMismatch,
    IndexOutOfBounds,
    Multiple,
    NotRunning
}

type InterpResult = Result<(), InterpError>;
const OK: InterpResult = Ok(());

trait Interp: Default + Sized + Debug {
    type Value: HasInterp<Interp = Self>;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult;

    fn interp(&self, frac: &AnimationCtx, val: &mut Self::Value) -> InterpResult;

    fn pod(self) -> Animation<Self::Value> {
        if self.is_noop() {
            Animation::Noop
        } else {
            Animation::Interp(self)
        }
    }

    fn is_noop(&self) -> bool;

    fn is_leaf() -> bool {
        false
    }

    fn select_animation_segment(self, idx: usize) -> Self;

    fn merge(self, other: Self) -> Self;

    fn build(start: Self::Value, end: Self::Value) -> Self;
}

trait HasInterp: Clone + Debug {
    type Interp: Interp<Value = Self>;
    fn tween(self, other: Self) -> Animation<Self> {
        Self::Interp::build(self, other).pod()
    }
}

#[derive(Debug)]
enum SelectAnim<Interp> {
    Single(AnimationSegmentId, Interp),
    Many(Vec<(AnimationSegmentId, Interp)>)
}

impl <TInterp: Interp> SelectAnim<TInterp>{
    fn prime(&mut self, val: &mut TInterp::Value)->InterpResult{
        match self{
            SelectAnim::Single(_, interp) => interp.prime(val),
            SelectAnim::Many(sels)=> sels.iter_mut().fold(OK, |r, (_, interp)|r.and_then(|_|interp.prime(val) )) // TODO do this based on status
        }
    }

    fn first(&mut self) ->&mut TInterp{
        match self{
            SelectAnim::Single(_, interp) => interp,
            SelectAnim::Many(selects) => {
                let idx = selects.len() - 1;
                &mut selects[idx].1
            }
        }
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut TInterp::Value) -> InterpResult {
        match self {
            SelectAnim::Single(cur_idx, interp) => {
                // TODO check if running
                let new_frac = AnimationCtx::new(*cur_idx, frac.segment_fractions);
                interp.interp(&new_frac, val)
            },
            SelectAnim::Many( selects )=>{
                // TODO check if running
                for (idx, interp) in selects{
                    let new_frac = AnimationCtx::new(*idx, frac.segment_fractions);
                    interp.interp(&new_frac, val)?
                }
                OK
            }
        }
    }

    fn append(self, other: Self)->Self{
        let mut vec = self.vec();
        vec.append(&mut other.vec());
        SelectAnim::Many( vec )
    }

    fn vec(self)->Vec<(AnimationSegmentId, TInterp)>{
        match self{
            SelectAnim::Single(ai, interp) => vec![(ai, interp)],
            SelectAnim::Many(vec) => vec,
        }
    }

    fn select_internal(self)->(AnimationSegmentId, TInterp){
        match self{
            SelectAnim::Single(ai, interp) => (ai, interp.select_animation_segment(ai)),
            SelectAnim::Many(vec) => {
                vec.into_iter().fold( (0, TInterp::default()), |(_, cur), (ai, item)| (ai, cur.merge(item)))
            }
        }
    }
}

#[derive(Debug)]
enum Animation<Value: HasInterp> {
    Noop,
    Interp(Value::Interp),
    Select(SelectAnim<Value::Interp>)
}

impl <Value: HasInterp> Default for Animation<Value>{
    fn default() -> Self {
        Animation::Noop
    }
}

impl<Value: HasInterp> Animation<Value> {
    fn is_noop(&self) -> bool {
        matches!(self, Animation::Noop)
    }

    fn prime(&mut self, val: &mut Value) -> InterpResult {
        match self {
            Animation::Noop => OK,
            Animation::Interp(interp) => interp.prime(val),
            Animation::Select(sa) => sa.prime(val),
        }
    }

    fn first(&mut self) ->&mut Value::Interp{
        if let Animation::Noop = self {
            *self = Animation::Interp(Value::Interp::default());
        }
        match self{
            Animation::Noop => unreachable!("Just ensured we aren't noop"),
            Animation::Interp(interp) => interp,
            Animation::Select(sa) => sa.first()
        }
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Value) -> InterpResult {
        match self {
            Animation::Noop => OK,
            Animation::Interp(interp) => interp.interp(frac, val),
            Animation::Select(sa) => sa.interp(frac, val)
        }
    }

    fn select_anim(self, idx: usize) -> Self {
        match self {
            Animation::Interp(interp) => Animation::Select(SelectAnim::Single(idx, interp)),
            s => s,
        }
    }

    fn merge(self, other: Animation<Value>) -> Self {

        fn wrap_anim<P: Interp>(p: P, idx: usize) -> Animation<P::Value> {
            if P::is_leaf() {
                Animation::Select(SelectAnim::Single(idx, p))
            } else {
                Animation::Interp(p)
            }
        }
        match (self, other) {
            (Animation::Noop, other) => other,
            (other, Animation::Noop) => other,
            (Animation::Select(sa1), Animation::Select(sa2)) => {
                //let (_, si1) = sa1.select_internal();
                //let (a2, si2) = sa2.select_internal();
                //wrap_anim(si2.merge(si1), a2)

                //TODO: descending merge
                Animation::Select( sa1.append(sa2) )
            }
            (Animation::Select(sa), Animation::Interp(p)) => {
                let (a1, si1) = sa.select_internal();
                wrap_anim(si1.merge(p), a1)
            }
            (Animation::Interp(p), Animation::Select(sa)) => {
                let (a2, si2) = sa.select_internal();
                wrap_anim(p.merge(si2), a2)
            },
            (_, other) => other,
        }
    }
}

#[derive(Debug)]
struct MapInterp<Value: HasInterp, Key: Hash + Eq> {
    to_prime: Vec<(Key, Value)>,
    interps: Vec<(Key, Animation<Value>)>,
}

impl <Value: HasInterp, Key: Hash + Eq + Clone> MapInterp<Value, Key>{
    fn for_key(&mut self, key: &Key)->&mut Animation<Value>{
        let idx = self.interps.iter().position(|(k, v)| *k == *key).unwrap_or_else(|| {
            let idx = self.interps.len();
            self.interps.push((key.clone(), Default::default()));
            idx
        });
        &mut self.interps[idx].1
    }
}

impl <Value: HasInterp, Key: Hash + Eq >  Default for MapInterp<Value, Key> {
    fn default() -> Self {
        MapInterp{to_prime: Default::default(), interps: Default::default() }
    }
}

impl<Value: HasInterp, Key: Hash + Eq> MapInterp<Value, Key> {
    pub fn new(to_prime: Vec<(Key, Value)>, interps: Vec<(Key, Animation<Value>)>) -> Self {
        MapInterp { to_prime, interps }
    }
}

impl<Value: HasInterp + EnterExit, Key: Eq + Hash + Clone + Debug> HasInterp
    for HashMap<Key, Value>
{
    type Interp = MapInterp<Value, Key>;
}

trait EnterExit {
    fn enter(&self) -> Self;
    fn exit(&self) -> Self;
}

impl<Value: HasInterp + EnterExit, Key: Debug + Hash + Eq + Clone> Interp
    for MapInterp<Value, Key>
{
    type Value = HashMap<Key, Value>;

    fn prime(&mut self, val: &mut HashMap<Key, Value>) -> InterpResult {
        for (k, v) in self.to_prime.drain(..) {
            val.insert(k, v);
        }
        for (k, i) in &mut self.interps {
            if let Some(val) = val.get_mut(k) {
                i.prime(val)?
            }
        }
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut HashMap<Key, Value>) -> InterpResult {
        let mut loop_err: Option<InterpError> = None;
        for (key, interp) in self.interps.iter() {
            let cur_err = val
                .get_mut(key)
                .map(|value| interp.interp(frac, value))
                .unwrap_or(Err(IndexOutOfBounds));

            match (&mut loop_err, cur_err) {
                (loop_err @ None, Err(c_e)) => *loop_err = Some(c_e),
                (Some(l_e), Err(c_e)) if *l_e != c_e => *l_e = InterpError::Multiple,
                _ => (),
            }
        }
        loop_err.map(Err).unwrap_or(OK)
    }

    fn is_noop(&self) -> bool {
        self.interps.is_empty()
    }

    fn select_animation_segment(self, a_idx: usize) -> Self {
        MapInterp {
            to_prime: self.to_prime,
            interps: self
                .interps
                .into_iter()
                .map(|(key, interp)| (key, interp.select_anim(a_idx)))
                .collect(),
        }
    }

    fn merge(self, other: MapInterp<Value, Key>) -> Self {
        let mut interps: HashMap<_, _> = self.interps.into_iter().collect();
        for (key, interp) in other.interps.into_iter() {
            let new_interp = if let Some(cur) = interps.remove(&key) {
                cur.merge(interp)
            } else {
                interp
            };
            if !new_interp.is_noop() {
                interps.insert(key, new_interp);
            }
        }

        let mut to_prime = self.to_prime;
        let mut o_to_prime = other.to_prime;
        to_prime.append(&mut o_to_prime);
        MapInterp {
            to_prime ,
            interps: interps.into_iter().collect(),
        }
    }

    fn build(start: HashMap<Key, Value>, end: HashMap<Key, Value>) -> Self {
        let mut matched_marks: HashMap<Key, (Option<Value>, Option<Value>)> = HashMap::new();

        for (key, value) in start.into_iter() {
            matched_marks
                .entry(key)
                .or_insert_with(|| (Some(value), None));
        }

        for (key, value) in end.into_iter() {
            matched_marks.entry(key).or_insert_with(|| (None, None)).1 = Some(value)
        }

        let mut interps = Vec::new();
        let mut to_prime = Vec::new();

        for (k, v) in matched_marks.into_iter() {
            match v {
                (Some(o), Some(n)) => {
                    interps.push((k, Value::Interp::build(o.clone(), n).pod()));
                }
                (None, Some(n)) => {
                    let e = n.enter();
                    interps.push((k.clone(), Value::Interp::build(e.clone(), n).pod()));
                    to_prime.push((k, e));
                }
                (Some(o), None) => {
                    let e = o.exit();
                    interps.push((k, Value::Interp::build(o.clone(), e).pod()));
                }
                _ => (),
            }
        }

        MapInterp { to_prime, interps }
    }
}

#[derive(Debug, Default)]
struct F64Interp {
    start: f64,
    end: f64,
}

impl F64Interp {
    fn interp_raw(start: f64, end: f64, frac: f64) -> f64 {
        start + (end - start) * frac
    }
}

impl HasInterp for f64 {
    type Interp = F64Interp;
}

impl Interp for F64Interp {
    type Value = f64;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut f64) -> InterpResult {
        *val = Self::interp_raw(self.start, self.end, frac.current());
        OK
    }

    fn is_noop(&self) -> bool {
        self.start == self.end
    }

    fn is_leaf() -> bool {
        true
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        self
    }

    fn merge(self, other: Self) -> Self {
        other
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Default)]
struct PointInterp {
    x: Animation<f64>,
    y: Animation<f64>,
}

impl HasInterp for Point {
    type Interp = PointInterp;
}

impl Interp for PointInterp {
    type Value = Point;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        self.x.prime(&mut val.x)?;
        self.y.prime(&mut val.y)
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Point) -> InterpResult {
        self.x.interp(frac, &mut val.x)?;
        self.y.interp(frac, &mut val.y)
    }

    fn is_noop(&self) -> bool {
        self.x.is_noop() && self.y.is_noop()
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        Self {
            x: self.x.select_anim(idx),
            y: self.y.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        PointInterp {
            x: self.x.merge(other.x),
            y: self.y.merge(other.y),
        }
    }

    fn build(old: Point, new: Point) -> PointInterp {
        PointInterp {
            x: old.x.tween(new.x),
            y: old.y.tween(new.y),
        }
    }
}

#[derive(Debug, Default)]
struct StringInterp {
    prefix: String,
    remove: String,
    add: String,
    steps: usize,
}

impl StringInterp {
    fn new(a: &str, b: &str) -> StringInterp {
        let prefix: String = a
            .chars()
            .zip(b.chars())
            .take_while(|(a, b)| a == b)
            .map(|v| v.0)
            .collect();
        let remove: String = a[prefix.len()..].into();
        let add: String = b[prefix.len()..].into();
        let steps = remove.len() + add.len();
        StringInterp {
            prefix,
            remove,
            add,
            steps,
        }
    }
}

impl HasInterp for String {
    type Interp = StringInterp;
}

impl Interp for StringInterp {
    type Value = String;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        // TODO: do this by modifying? Can't assume that calls are in order though
        let step = ((self.steps as f64) * frac.current()) as isize;
        let r_len = self.remove.len() as isize;
        let step = step - r_len;
        if step > 0 {
            *val = format!("{}{}", self.prefix, &self.add[..(step as usize)]).into()
        } else if step < 0 {
            *val = format!("{}{}", self.prefix, &self.remove[..(-step) as usize]).into()
        } else {
            *val = self.prefix.clone()
        }
        OK
    }



    fn is_noop(&self) -> bool {
        self.steps == 0
    }

    fn is_leaf() -> bool {
        true
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        self
    }

    fn merge(self, other: Self) -> Self {
        other
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self::new(&start, &end)
    }
}

#[derive(Debug, Default)]
struct TextMarkInterp {
    txt: Animation<String>,
    size: Animation<f64>,
    point: Animation<Point>,
}

impl HasInterp for TextMark {
    type Interp = TextMarkInterp;
}

impl Interp for TextMarkInterp {
    type Value = TextMark;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        self.txt.prime(&mut val.txt)?;
        self.size.prime(&mut val.size)?;
        self.point.prime(&mut val.point)
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut TextMark) -> InterpResult {
        self.txt.interp(frac, &mut val.txt)?;
        self.size.interp(frac, &mut val.size)?;
        self.point.interp(frac, &mut val.point)
    }

    fn is_noop(&self) -> bool {
        self.point.is_noop() && self.size.is_noop() && self.txt.is_noop()
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        let TextMarkInterp { txt, size, point } = self;
        TextMarkInterp {
            txt: txt.select_anim(idx),
            size: size.select_anim(idx),
            point: point.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        let TextMarkInterp { txt, size, point } = self;
        let TextMarkInterp {
            txt: txt2,
            size: size2,
            point: point2,
        } = other;
        TextMarkInterp {
            txt: txt.merge(txt2),
            size: size.merge(size2),
            point: point.merge(point2),
        }
    }

    fn build(o: Self::Value, n: Self::Value) -> Self {
        TextMarkInterp {
            txt: o.txt.tween(n.txt),
            size: o.size.tween(n.size),
            point: o.point.tween(n.point),
        }
    }
}

#[derive(Debug)]
enum MarkShapeInterp {
    Rect(Animation<Point>, Animation<Point>),
    Line(Animation<Point>, Animation<Point>),
    Text(Animation<TextMark>),
    Noop,
}

impl Default for MarkShapeInterp{
    fn default() -> Self {
        MarkShapeInterp::Noop
    }
}

impl HasInterp for MarkShape {
    type Interp = MarkShapeInterp;
}

impl Interp for MarkShapeInterp {
    type Value = MarkShape;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        match (self, val) {
            (MarkShapeInterp::Rect(o, other), MarkShape::Rect(r)) => {
                // Pointless
                OK
            }
            (MarkShapeInterp::Line(o, other), MarkShape::Line(l)) => {
                o.prime(&mut l.p0)?;
                other.prime(&mut l.p1)?;
                OK
            }
            (MarkShapeInterp::Text(t_interp), MarkShape::Text(t)) => {
                t_interp.prime(t)?;
                OK
            }
            _ => Err(InterpError::ValueMismatch),
        }
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut MarkShape) -> InterpResult {
        match (self, val) {
            (MarkShapeInterp::Rect(o, other), MarkShape::Rect(r)) => {
                // TODO: Do coords not points
                let (mut o_p, mut other_p) = (r.origin(), Point::new(r.x1, r.y1));
                o.interp(frac, &mut o_p)?;
                other.interp(frac, &mut other_p)?;
                *r = Rect::from_points(o_p, other_p);
                OK
            }
            (MarkShapeInterp::Line(o, other), MarkShape::Line(l)) => {
                o.interp(frac, &mut l.p0)?;
                other.interp(frac, &mut l.p1)?;
                OK
            }
            (MarkShapeInterp::Text(t_interp), MarkShape::Text(t)) => {
                t_interp.interp(frac, t)?;
                OK
            }
            _ => Err(InterpError::ValueMismatch),
        }
    }

    fn is_noop(&self) -> bool {
        match self {
            MarkShapeInterp::Rect(orig, other) => orig.is_noop() && other.is_noop(),
            MarkShapeInterp::Line(start, end) => start.is_noop() && end.is_noop(),
            MarkShapeInterp::Text(text) => text.is_noop(),
            MarkShapeInterp::Noop => true,
        }
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        match self {
            MarkShapeInterp::Rect(orig, other) => {
                MarkShapeInterp::Rect(orig.select_anim(idx), other.select_anim(idx))
            }
            MarkShapeInterp::Line(start, end) => {
                MarkShapeInterp::Line(start.select_anim(idx), end.select_anim(idx))
            }
            MarkShapeInterp::Text(text) => MarkShapeInterp::Text(text.select_anim(idx)),
            other => other,
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (MarkShapeInterp::Rect(orig1, other1), MarkShapeInterp::Rect(orig2, other2)) => {
                MarkShapeInterp::Rect(orig1.merge(orig2), other1.merge(other2))
            }
            (MarkShapeInterp::Line(s1, e1), MarkShapeInterp::Line(s2, e2)) => {
                MarkShapeInterp::Line(s1.merge(s2), e1.merge(e2))
            }
            (MarkShapeInterp::Text(t1), MarkShapeInterp::Text(t2)) => {
                MarkShapeInterp::Text(t1.merge(t2))
            }
            (_, other) => other,
        }
    }

    fn build(old: MarkShape, new: MarkShape) -> MarkShapeInterp {
        fn other_point(r: &Rect) -> Point {
            Point::new(r.x1, r.y1)
        }

        match (old, new) {
            (o, n) if o.same(&n) => MarkShapeInterp::Noop,
            (MarkShape::Rect(o), MarkShape::Rect(n)) => MarkShapeInterp::Rect(
                o.origin().tween(n.origin()),
                other_point(&o).tween(other_point(&n)),
            ),
            (MarkShape::Line(o), MarkShape::Line(n)) => {
                MarkShapeInterp::Line(o.p0.tween(n.p0), o.p1.tween(n.p1))
            }
            (MarkShape::Text(o), MarkShape::Text(n)) => MarkShapeInterp::Text(o.tween(n)),
            _ => MarkShapeInterp::Noop,
        }
    }
}

#[derive(Debug)]
enum ColorInterp {
    Rgba(Animation<f64>, Animation<f64>, Animation<f64>, Animation<f64>),
    Noop
}

impl Default for ColorInterp{
    fn default() -> Self {
        ColorInterp::Noop
    }
}

impl HasInterp for Color {
    type Interp = ColorInterp;
}

impl Interp for ColorInterp {
    type Value = Color;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Color) -> InterpResult {
        match self {
            ColorInterp::Rgba(ri, gi, bi, ai) => {
                let (mut r, mut g, mut b, mut a) = val.as_rgba();
                ri.interp(frac, &mut r)?;
                gi.interp(frac, &mut g)?;
                bi.interp(frac, &mut b)?;
                ai.interp(frac, &mut a)?;

                // TODO: mutate this?
                *val = Color::rgba(r, g, b, a);
                OK
            }
            ColorInterp::Noop => OK
        }
    }

    fn is_noop(&self) -> bool {
        match self {
            ColorInterp::Rgba(r, g, b, a) => {
                r.is_noop() && g.is_noop() && b.is_noop() && a.is_noop()
            }
            ColorInterp::Noop => true
        }
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        match self {
            ColorInterp::Rgba(r, g, b, a) => ColorInterp::Rgba(
                r.select_anim(idx),
                g.select_anim(idx),
                b.select_anim(idx),
                a.select_anim(idx),
            ),
            ColorInterp::Noop => self
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (ColorInterp::Rgba(r, g, b, a), ColorInterp::Rgba(r1, g1, b1, a1)) => {
                ColorInterp::Rgba(r.merge(r1), g.merge(g1), b.merge(b1), a.merge(a1))
            }
            (ColorInterp::Noop, other)=>other,
            (s, ColorInterp::Noop)=>s
        }
    }

    fn build(old: Color, new: Color) -> ColorInterp {
        let (r, g, b, a) = old.as_rgba();
        let (r2, g2, b2, a2) = new.as_rgba();

        ColorInterp::Rgba(
            F64Interp::build(r, r2).pod(),
            F64Interp::build(g, g2).pod(),
            F64Interp::build(b, b2).pod(),
            F64Interp::build(a, a2).pod(),
        )
    }
}

#[derive(Debug, Default)]
struct MarkInterp {
    shape: Animation<MarkShape>,
    color: Animation<Color>,
}

impl HasInterp for Mark {
    type Interp = MarkInterp;
}

impl Interp for MarkInterp {
    type Value = Mark;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Mark) -> InterpResult {
        self.shape.interp(frac, &mut val.shape)?;
        self.color.interp(frac, &mut val.color)?;
        OK
    }

    fn is_noop(&self) -> bool {
        self.shape.is_noop() && self.color.is_noop()
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        let MarkInterp { shape, color } = self;
        MarkInterp {
            shape: shape.select_anim(idx),
            color: color.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        let MarkInterp { shape, color } = self;
        let MarkInterp {
            shape: s1,
            color: c1,
        } = other;
        MarkInterp {
            shape: shape.merge(s1),
            color: color.merge(c1),
        }
    }

    fn build(old: Mark, new: Mark) -> Self {
        MarkInterp {
            shape: MarkShapeInterp::build(old.shape, new.shape).pod(),
            color: ColorInterp::build(old.color, new.color).pod(),
        }
    }
}

impl EnterExit for Mark {
    fn enter(&self) -> Self {
        let shape = match &self.shape {
            MarkShape::Rect(r) => MarkShape::Rect(Rect::from_center_size(r.center(), Size::ZERO)),
            MarkShape::Line(l) => {
                let arr = [AnimationSegmentState::new(0.0, 0.5)];
                let mut mid = l.p0;
                l.p0.tween(l.p1).interp(&AnimationCtx::new(0, &arr), &mut mid);
                MarkShape::Line(Line::new(mid, mid))
            }
            s => s.clone(),
        };
        Mark::new(
            self.id,
            shape,
            self.color.clone().with_alpha(0.),
            self.hover.clone(),
        )
    }

    fn exit(&self) -> Self {
        self.enter()
    }
}

impl Mark {
    pub fn new(id: MarkId, shape: MarkShape, color: Color, hover: Option<Color>) -> Self {
        Mark {
            id,
            shape,
            color: color.clone(),
            original: color,
            hover,
        }
    }

    pub fn hit(&self, pos: Point) -> bool {
        match self.shape {
            MarkShape::Rect(r) => r.contains(pos),
            MarkShape::Line(l) => {
                let (_, d2) = l.nearest(pos, 1.0);
                d2 < 1.0
            }
            _ => false,
        }
    }

    pub fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        // This should be done as some interpolation of the mark before paint? Maybe
        // let color = if Some(self.id) == *focus {
        //     self.hover.as_ref().unwrap_or(&self.color)
        // } else {
        //     &self.color
        // };
        let color = &self.color;
        match &self.shape {
            MarkShape::Rect(r) => {
                ctx.stroke(r, &Color::BLACK, 1.0);
                ctx.fill(r, color);
            }
            MarkShape::Line(l) => {
                ctx.stroke(l, color, 1.0);
            }
            MarkShape::Text(t) => {
                // Not saving the text layout at the moment - as the color is embedded into it and we are deriving it.

                if let Ok(tl) = ctx
                    .text()
                    .new_text_layout(&t.txt)
                    .font(t.font_fam.clone(), t.size)
                    .text_color(color.clone())
                    .build()
                {
                    ctx.with_save(|ctx| {
                        // Flip the coordinates back to draw text
                        ctx.transform(
                            Affine::translate(
                                t.point.to_vec2()
                                    - Vec2::new(
                                        tl.size().width / 2.,
                                        0.0, /*-tl.size().height */
                                    ),
                            ) * Affine::FLIP_Y,
                        );
                        ctx.draw_text(&tl, Point::ORIGIN);
                    });
                }
            }
        }
    }
}

#[derive(Debug, Data, Clone, PartialEq)]
pub struct TextMark {
    txt: String,
    font_fam: FontFamily,
    size: f64,
    point: Point,
}

impl TextMark {
    pub fn new(txt: String, font_fam: FontFamily, size: f64, point: Point) -> Self {
        TextMark {
            txt,
            font_fam,
            size,
            point,
        }
    }
}

#[derive(Debug, Data, Clone)]
pub enum MarkShape {
    Rect(Rect),
    Line(Line),
    Text(TextMark),
}

// A data series
#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SeriesId(pub usize);

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct DatumId {
    pub series: SeriesId,
    pub idx: LogIdx,
}

impl DatumId {
    pub fn new(series: SeriesId, idx: LogIdx) -> Self {
        DatumId { series, idx }
    }
} // Series, Generator etc

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct AxisName(pub &'static str);

impl Data for AxisName {
    fn same(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum TickLocator {
    Ordinal(usize),
    Persistent(PersistentId),
    U64Bits(u64),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StateName(pub &'static str);

impl Data for StateName {
    fn same(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum MarkId {
    Datum(DatumId),
    AxisDomain(AxisName),
    Tick(AxisName, TickLocator),
    TickText(AxisName, TickLocator),
    StateMark(StateName, u32),
}

#[derive(Clone, Debug)]
pub struct DrawableAxis {
    marks: Vec<Mark>,
}

impl Data for DrawableAxis {
    fn same(&self, other: &Self) -> bool {
        self.marks.len() == other.marks.len()
            && self
                .marks
                .iter()
                .zip(other.marks.iter())
                .all(|(o, n)| o.same(n))
    }
}

impl DrawableAxis {
    pub fn new(marks: Vec<Mark>) -> Self {
        DrawableAxis { marks }
    }
}

#[derive(Debug)]
pub enum VisEvent {
    MouseEnter(MarkId),
    MouseOut(MarkId),
}

pub trait Visualization {
    type Input: Data;
    type State: Default + Data + Debug;
    type Layout;

    fn layout(&mut self, data: &Self::Input, size: Size) -> Self::Layout;
    fn event(
        &self,
        data: &mut Self::Input,
        layout: &Self::Layout,
        state: &mut Self::State,
        event: &VisEvent,
    );

    fn layout_marks(&self, layout: &Self::Layout) -> Vec<DrawableAxis>;
    fn state_marks(
        &self,
        data: &Self::Input,
        layout: &Self::Layout,
        state: &Self::State,
    ) -> Vec<Mark>;
    fn data_marks(&mut self, data: &Self::Input, layout: &Self::Layout) -> Vec<Mark>;
}

#[derive(Clone, Debug, Default)]
struct VisMarks {
    // TODO: slotmap?
    marks: HashMap<MarkId, Mark>,
}

impl VisMarks {
    fn find_mark(&self, pos: Point) -> Option<&Mark> {
        self.marks.values().filter(|mark| mark.hit(pos)).next()
    }

    fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        for (_, mark) in self.marks.iter().sorted_by_key(|(k, v)| k.clone()) {
            mark.paint(ctx, focus);
        }
    }

    fn build(
        layout_marks: Vec<DrawableAxis>,
        state_marks: Vec<Mark>,
        data_marks: Vec<Mark>,
    ) -> VisMarks {
        let mut marks = HashMap::new();
        for mark in state_marks
            .into_iter()
            .chain(data_marks.into_iter())
            .chain(layout_marks.into_iter().flat_map(|x| x.marks))
        {
            marks.insert(mark.id, mark);
        }

        VisMarks { marks }
    }
}

#[derive(Debug, Default)]
struct VisMarksInterp {
    marks: Animation<HashMap<MarkId, Mark>>,
}

impl VisMarksInterp {
    pub fn new(marks: Animation<HashMap<MarkId, Mark>>) -> Self {
        VisMarksInterp { marks }
    }
}

impl HasInterp for VisMarks {
    type Interp = VisMarksInterp;
}

impl Interp for VisMarksInterp {
    type Value = VisMarks;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        self.marks.prime(&mut val.marks)
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut VisMarks) -> InterpResult {
        self.marks.interp(frac, &mut val.marks)
    }

    fn is_noop(&self) -> bool {
        self.marks.is_noop()
    }

    fn select_animation_segment(self, idx: usize) -> Self {
        VisMarksInterp {
            marks: self.marks.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            marks: self.marks.merge(other.marks),
        }
    }

    fn build(old: VisMarks, new: VisMarks) -> Self {
        Self {
            marks: MapInterp::build(old.marks, new.marks).pod(),
        }
    }
}

#[derive(Debug, PartialEq)]
enum AnimationSegmentStatus{
    Pending,
    Waiting, // Move start time in here
    Running,
    Done
}

enum CustomAnimationCurve {
    Function(fn(f64) -> f64),
    Boxed(Box<dyn Fn(f64) -> f64>)
}

impl CustomAnimationCurve{
    fn translate(&self, t: f64) ->f64 {
        match self {
            CustomAnimationCurve::Function(f) => f(t),
            CustomAnimationCurve::Boxed(f) => f(t)
        }
    }
}

impl Debug for CustomAnimationCurve{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self{
            CustomAnimationCurve::Function(f)=> {
                formatter.debug_struct("CustomAnimationCurve::Function").field("f", f).finish()
            }
            CustomAnimationCurve::Boxed(_) => {
                formatter.debug_struct("CustomAnimationCurve::Closure").finish()
            }
        }

    }
}

#[derive(Debug)]
enum AnimationCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    OutElastic,
    OutBounce,
    OutSine,
    //CubicBezier
    Custom(CustomAnimationCurve)
}

impl AnimationCurve {
    fn translate(&self, t: f64) ->f64{
        use std::f64::consts::PI;
        match self {
            AnimationCurve::Linear => t,
            AnimationCurve::EaseIn => t * t,
            AnimationCurve::EaseOut => t * (2.0 - t),
            AnimationCurve::EaseInOut => {
                let t = t * 2.0;
                if t < 1. {
                    0.5 * t * t
                } else {
                    let t = t - 1.;
                    -0.5 * (t * (t - 2.) - 1.)
                }
            }
            AnimationCurve::OutElastic => {
                let p = 0.3;
                let s = p / 4.0;

                if t < 0.001 {
                    0.
                }else if t > 0.999 {
                    1.
                }else {
                    2.0f64.powf(-10.0 * t) * ((t - s) * (2.0 * PI) / p).sin() + 1.0
                }
            }
            AnimationCurve::OutSine => {
                (t * PI * 0.5).sin()
            }
            AnimationCurve::OutBounce => {
                if t < (1. / 2.75) {
                    7.5625 * t * t
                }else if t < (2. / 2.75) {
                    let t = t - (1.5 / 2.75);
                    7.5625 * t * t + 0.75
                }else if t < (2.5 / 2.75) {
                    let t = t - (2.25 / 2.75);
                    7.5625 * t * t + 0.9375
                }else {
                    let t = t - (2.625 / 2.75);
                    7.5625 * t * t + 0.984375
                }
            },
            AnimationCurve::Custom(c) => c.translate(t)
        }
    }
}

#[derive(Debug)]
struct AnimationSegment {
    start_nanos: f64,
    dur_nanos: f64,
    curve: AnimationCurve,
    status: AnimationSegmentStatus
}

impl AnimationSegment {
    pub fn new(start_nanos: f64, dur_nanos: f64, curve: AnimationCurve, status: AnimationSegmentStatus) -> Self {
        AnimationSegment {
            start_nanos,
            dur_nanos,
            curve,
            status
        }
    }
}

type AnimationSegmentId = usize;

#[derive(Eq, PartialEq, Hash, Debug)]
enum AnimationEvent{
    Named(&'static str),
    SegmentEnded(AnimationSegmentId)
}

#[derive(Default)]
struct Animator<T : HasInterp> {
    cur_nanos: f64,
    pending_starts: HashMap<AnimationEvent, Vec<AnimationSegmentId>>,
    // slot map?
    segments: Vec<AnimationSegment>,
    pod: Animation<T>,
}

impl <T: HasInterp> Animator<T> {
    pub fn advance(&mut self, nanos: f64, current: &mut T) -> InterpResult {
        if self.segments.len() == 0 {
            return InterpResult::Err(InterpError::NotRunning)
        }
        self.cur_nanos += nanos;
        let cur_nanos = self.cur_nanos;
        let fracs: Vec<_> = self.segments.iter_mut().map(|segment|{
            let since_start = cur_nanos - segment.start_nanos;
            if AnimationSegmentStatus::Waiting == segment.status && since_start >= 0. {
                segment.status = AnimationSegmentStatus::Running
            }
            match &mut segment.status {
                AnimationSegmentStatus::Waiting | AnimationSegmentStatus::Pending => AnimationSegmentState::START,
                stat@AnimationSegmentStatus::Running => {
                    let frac = since_start / segment.dur_nanos;
                    if frac > 1.0 {
                        *stat = AnimationSegmentStatus::Done;
                        AnimationSegmentState::END
                    } else {
                        AnimationSegmentState::new(frac, segment.curve.translate(frac).max(0.).min(1.))
                    }
                },
                AnimationSegmentStatus::Done => AnimationSegmentState::END
            }
        }).collect();

        let frac = AnimationCtx::new(0, &fracs[..]);
        let res = self.pod.interp(&frac, current);
        let done = self.segments.iter().all(|a| a.status == AnimationSegmentStatus::Done);
        if done {
            self.segments.clear();
            self.pod = Animation::Noop;
        }
        res
    }

    pub fn event(&mut self, event: AnimationEvent){
        // TODO: with repeating segments do not remove
        if let Some(ids) = self.pending_starts.remove(&event){
            for id in ids{
                if let Some(seg) = self.segments.get_mut(id){
                    seg.status = AnimationSegmentStatus::Waiting;
                    seg.start_nanos = self.cur_nanos;
                    log::info!("Starting pending anim {} {:?}\n {:#?}", id, seg, self.pod);
                }
            }
        }
    }

    pub fn running(&self)->bool{
        // TODO: If we had waiting ones we could return a minimum time until one had to start
        // then use a timer to get it
        !self.segments.iter().all(|s|s.status == AnimationSegmentStatus::Pending)
    }

    pub fn add_animation_segment(&mut self, from_now_nanos: u64, dur_nanos: u64, curve: AnimationCurve, after:Option<AnimationEvent>)->usize{
        let anim_idx = self.segments.len();
        let start = self.cur_nanos + from_now_nanos as f64;
        let status = if after.is_some(){
            AnimationSegmentStatus::Pending
        } else if from_now_nanos > 0 { AnimationSegmentStatus::Waiting } else { AnimationSegmentStatus::Running };
        self.segments.push(AnimationSegment::new(
            start as f64,
            dur_nanos.max(1) as f64,
             curve,
            status
        ));
        if let Some(after) = after {
            self.pending_starts.entry(after).or_insert(vec![]).push(anim_idx);
        }
        return anim_idx;
    }

    pub fn add_animation(&mut self, interp: Animation<T>, from_now_nanos: u64, dur_nanos: u64, curve: AnimationCurve, after: Option<AnimationEvent>, current: &mut T) -> Result<(), InterpError> {
        let mut interp = interp;
        interp.prime(current).and_then(|()| {
            let anim_idx = self.add_animation_segment(from_now_nanos, dur_nanos, curve, after);
            let mut temp = Animation::Noop;
            std::mem::swap(&mut temp, &mut self.pod);
            self.pod = temp.merge(interp.select_anim(anim_idx));
            OK
        })
    }
}

struct VisInner<VP: Visualization> {
    layout: VP::Layout,
    state: VP::State,
    animator: Animator<VisMarks>,
    current: VisMarks,
    transform: Affine,
    focus: Option<MarkId>,
    phantom_vp: PhantomData<VP>,
}

impl<VP: Visualization> VisInner<VP> {
    pub fn new(layout: VP::Layout, state: VP::State, animator: Animator<VisMarks>, current: VisMarks, transform: Affine) -> Self {
        VisInner {
            layout,
            state,
            animator,
            current,
            transform,
            focus: None,
            phantom_vp: Default::default(),
        }
    }
}

pub struct Vis<V: Visualization> {
    visual: V,
    inner: Option<VisInner<V>>,
}

impl<V: Visualization> Vis<V> {
    const UNHOVER: AnimationEvent = AnimationEvent::Named("vis:unhover");


    pub fn new(visual: V) -> Self {
        Vis {
            visual,
            inner: None,
        }
    }

    fn ensure_inner(&mut self, data: &V::Input, size: Size) -> &mut VisInner<V> {
        if self.inner.is_none() {
            let state: V::State = Default::default();

            let layout = self.visual.layout(data, size);
            let state_marks = self.visual.state_marks(data, &layout, &state);
            let data_marks = self.visual.data_marks(data, &layout);
            let layout_marks = self.visual.layout_marks(&layout);

            let current = VisMarks::build(layout_marks, state_marks, data_marks);
            let animator = Default::default();

            self.inner = Some(VisInner::new(
                layout,
                state,
                animator,
                current,
                Affine::FLIP_Y * Affine::translate(Vec2::new(0., -size.height)),
            ));
        }
        self.inner.as_mut().unwrap()
    }

    fn start_transition(&mut self, size: Size, data: &V::Input) {
        if let Some(inner) = &mut self.inner {
            inner.layout = self.visual.layout(data, size);

            let destination = VisMarks::build(
                self.visual.layout_marks(&inner.layout),
                self.visual.state_marks(data, &inner.layout, &inner.state),
                self.visual.data_marks(data, &inner.layout),
            );

            let interp = VisMarksInterp::build(inner.current.clone(), destination).pod();
            inner.animator.add_animation(interp, 0, 250 * 1_000_000, AnimationCurve::EaseInOut, None, &mut inner.current);
        };
    }

}



impl<V: Visualization> Widget<V::Input> for Vis<V> {

    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut V::Input, env: &Env) {
        self.ensure_inner(data, ctx.size());
        let inner = self.inner.as_mut().unwrap();
        let old_state: V::State = inner.state.clone();
        let VisInner {animator, current, ..} = inner;

        match event {
            Event::MouseMove(me) => {
                if let Some(mark) = current.find_mark(inner.transform.inverse() * me.pos) {
                    if inner.focus != Some(mark.id) {
                        self.visual.event(
                            data,
                            &inner.layout,
                            &mut inner.state,
                            &VisEvent::MouseEnter(mark.id),
                        );
                        inner.focus = Some(mark.id);


                        if let Some(hover) = &mark.hover {
                            let hover_idx = animator.add_animation_segment(0, 250 * 1_000_000, AnimationCurve::EaseIn, None);
                            let color_change = mark.color.clone().tween(hover.clone()).select_anim(hover_idx);

                            let unhover_idx = animator.add_animation_segment( 0, 2500 * 1_000_000, AnimationCurve::EaseOut,  Some(Self::UNHOVER));
                            let change_back = hover.clone().tween(mark.original.clone()).select_anim(unhover_idx);

                            animator.pod.first().marks.first().for_key(&mark.id).first().color = color_change.merge(change_back);

                        }
                    }
                } else {
                    match &mut inner.focus {
                        Some(focus) => {
                            self.visual.event(
                                data,
                                &inner.layout,
                                &mut inner.state,
                                &VisEvent::MouseOut(*focus),
                            );
                            animator.event(Self::UNHOVER)
                        }
                        None => {}
                    }
                    if inner.focus.is_some() {
                        inner.focus = None;
                    }
                }

            }
            _ => {}
        }

        if inner.animator.running(){
            ctx.request_anim_frame()
        }

       // if !old_state.same(&inner.state) {
         //   self.start_transition(ctx.size(), data);
        //    ctx.request_anim_frame();
        //}
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &V::Input, env: &Env) {
        if let (LifeCycle::AnimFrame(nanos), Some(VisInner { animator, current, .. })) =
            (event, &mut self.inner)
        {
            let res = animator.advance((*nanos) as f64, current);
            if let Result::Err(e) = res{
                log::warn!("InterpError running animator {:?}", e);
            }

            if animator.running() {
                ctx.request_anim_frame();
            }
            ctx.request_paint()
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &V::Input, data: &V::Input, env: &Env) {
        if !data.same(old_data) {
            self.start_transition(ctx.size(), data);
            ctx.request_anim_frame();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &V::Input,
        _env: &Env,
    ) -> Size {
        self.inner = None;
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &V::Input, env: &Env) {
        let size = ctx.size();

        let state = self.ensure_inner(data, size);
        ctx.with_save(|ctx| {
            ctx.transform(state.transform);
            state.current.paint(ctx, &state.focus);
        });
    }
}

type PersistentId = usize;

pub struct OffsetSource<T, Id> {
    val_to_id: HashMap<T, Id>,
}

impl<T: Hash + Eq, Id> Default for OffsetSource<T, Id> {
    fn default() -> Self {
        Self {
            val_to_id: Default::default(),
        }
    }
}

impl<T: Clone + Hash + Eq, Id: From<usize> + Into<usize> + Clone> OffsetSource<T, Id> {
    pub fn offset(&mut self, item: &T) -> Id {
        let next: Id = self.val_to_id.len().into();
        self.val_to_id.entry(item.clone()).or_insert(next).clone()
    }
}

pub struct BandScaleFactory<T> {
    name: AxisName,
    offset_source: OffsetSource<T, PersistentId>,
}

impl<T: Clone + Ord + Hash + Display> BandScaleFactory<T> {
    pub fn new(name: AxisName) -> Self {
        BandScaleFactory {
            name,
            offset_source: Default::default(),
        }
    }

    pub fn make_scale(
        &mut self,
        range: F64Range,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64,
    ) -> BandScale<T> {
        BandScale::new(
            self.name,
            range,
            &mut self.offset_source,
            bands_it,
            padding_ratio,
        )
    }
}

pub struct BandScale<T: Clone + Ord + Hash + Display> {
    name: AxisName,
    range: F64Range,
    bands: Vec<(T, PersistentId)>,
    bands_lookup: HashMap<T, usize>,
    range_per_band: f64,
    half_padding: f64,
}

impl<T: Clone + Ord + Hash + Display> BandScale<T> {
    pub fn new(
        name: AxisName,
        range: F64Range,
        offset_source: &mut OffsetSource<T, PersistentId>,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64,
    ) -> Self {
        let mut uniq = BTreeSet::new();
        for item in bands_it {
            uniq.insert(item);
        }
        let bands: Vec<_> = uniq
            .iter()
            .map(|band| {
                let persistent_id = offset_source.offset(&band);
                (band.clone(), persistent_id)
            })
            .collect();
        let bands_lookup: HashMap<_, _> =
            uniq.into_iter().enumerate().map(|(i, v)| (v, i)).collect();
        let range_per_band = range.distance() / (bands.len() as f64);
        let half_padding = padding_ratio * range_per_band / 2.;
        BandScale {
            name,
            range,
            bands,
            bands_lookup,
            range_per_band,
            half_padding,
        }
    }

    pub fn range_val(&self, domain_val: &T) -> F64Range {
        if let Some(idx) = self.bands_lookup.get(domain_val) {
            let start = self.range.0 + ((*idx as f64) * self.range_per_band);
            F64Range(
                start + self.half_padding,
                start + self.range_per_band - self.half_padding,
            )
        } else {
            F64Range(0., 0.) // todo : propagate option?
        }
    }

    pub fn make_axis(&self) -> DrawableAxis {
        let mut marks = Vec::new();
        let line_y = 30.0;
        let tick_extent = line_y - 8.;
        let label_top = tick_extent - 2.;
        marks.push(Mark::new(
            MarkId::AxisDomain(self.name),
            MarkShape::Line(Line::new((self.range.0, line_y), (self.range.1, line_y))),
            Color::WHITE,
            None,
        ));
        for (v, p_id) in self.bands.iter() {
            let tick_loc = TickLocator::Persistent(*p_id);
            let b_mid = self.range_val(v).mid();
            marks.push(Mark::new(
                MarkId::Tick(self.name, tick_loc),
                MarkShape::Line(Line::new((b_mid, tick_extent), (b_mid, line_y))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::TickText(self.name, tick_loc),
                MarkShape::Text(TextMark::new(
                    v.to_string(),
                    Default::default(),
                    12.0,
                    Point::new(b_mid, label_top),
                )),
                Color::WHITE,
                None,
            ));
        }
        DrawableAxis::new(marks)
    }
}

pub trait LinearValue: Clone + Ord + Display + Default {
    fn as_f64(&self) -> f64;
    fn from_f64(val: f64) -> Self;
}

impl LinearValue for u32 {
    fn as_f64(&self) -> f64 {
        *self as f64
    }

    fn from_f64(val: f64) -> Self {
        val as u32
    }
}

#[derive(Copy, Clone, Debug, Data)]
pub struct F64Range(pub f64, pub f64);

lazy_static! {
    static ref E10: f64 = 50.0_f64.sqrt();
    static ref E5: f64 = 10.0_f64.sqrt();
    static ref E2: f64 = 2.0_f64.sqrt();
}

impl F64Range {
    pub fn distance(self) -> f64 {
        self.1 - self.0
    }

    pub fn mid(self) -> f64 {
        (self.0 + self.1) / 2.
    }

    fn step_size(self, count: usize) -> f64 {
        let step = self.distance() / count.max(0) as f64;
        let power = (step.ln() / LN_10).floor();
        let error = step / 10.0_f64.powf(power);

        let factor = if error >= *E10 {
            10.
        } else if error >= *E5 {
            5.
        } else if error >= *E2 {
            2.
        } else {
            1.
        };

        if power >= 0. {
            factor * 10.0_f64.powf(power)
        } else {
            -(10.0_f64.powf(-power)) / factor
        }
    }

    // Fairly close to what D3 does here https://github.com/d3/d3-scale/blob/master/src/linear.js
    // TODO: decrementing ranges
    fn nice(self, count: usize) -> (F64Range, f64) {
        let max_iter = 10;
        let mut pre_step: f64 = NAN;
        let mut current = self;

        for _ in 0..max_iter {
            let step = current.step_size(count);
            if step == pre_step {
                break;
            } else {
                let F64Range(start, stop) = current;
                current = F64Range((start / step).floor() * step, (stop / step).ceil() * step);
            }
            pre_step = step;
        }
        (current, pre_step)
    }

    fn include_zero(self, inc: bool) -> Self {
        if inc {
            // TODO: negative / flipped
            F64Range(self.0.min(0.), self.1.max(0.))
        } else {
            self
        }
    }
}

pub struct LinearScale<T: LinearValue> {
    name: AxisName,
    pub range: F64Range,
    domain_range: F64Range,
    multiplier: f64,
    ticks: usize,
    tick_step: f64,
    phantom_t: PhantomData<T>,
}

impl<T: LinearValue> LinearScale<T> {
    pub fn new(
        name: AxisName,
        range: F64Range,
        domain_iter: &mut impl Iterator<Item = T>,
        nice: bool,
        ticks_goal: Option<usize>,
        zero: bool,
    ) -> Self {
        let ticks = ticks_goal.unwrap_or(10);
        let (start, stop) = domain_iter
            .minmax()
            .into_option()
            .unwrap_or_else(|| (Default::default(), Default::default()));
        let domain_range = F64Range(start.as_f64(), stop.as_f64()).include_zero(zero);
        let (domain_range, tick_step) = if nice {
            domain_range.nice(ticks)
        } else {
            (domain_range, domain_range.step_size(ticks))
        };
        let domain_dist = domain_range.distance();

        let ticks = (domain_dist / tick_step).ceil() as usize;
        let multiplier = if domain_dist == 0.0 {
            0.0
        } else {
            range.distance() / domain_dist
        };
        LinearScale {
            name,
            range,
            domain_range,
            multiplier,
            ticks,
            tick_step,
            phantom_t: Default::default(),
        }
    }

    pub fn range_val(&self, domain_val: &T) -> f64 {
        self.range_val_raw(domain_val.as_f64())
    }

    pub fn range_val_raw(&self, domain_float: f64) -> f64 {
        self.range.0 + self.multiplier * (domain_float - self.domain_range.0)
    }

    pub fn make_axis(&self) -> DrawableAxis {
        let mut marks = Vec::new();
        let line_x = 30.0;
        let tick_extent = line_x - 8.;
        let label_x = tick_extent - 2.;
        marks.push(Mark::new(
            MarkId::AxisDomain(self.name),
            MarkShape::Line(Line::new((line_x, self.range.0), (line_x, self.range.1))),
            Color::WHITE,
            None,
        ));

        for step in 0..=self.ticks {
            let d_v = self.domain_range.0 + self.tick_step * (step as f64);
            let value = T::from_f64(d_v);

            let r_v = self.range_val_raw(d_v);
            marks.push(Mark::new(
                MarkId::Tick(self.name, TickLocator::U64Bits(d_v.to_bits())),
                MarkShape::Line(Line::new((tick_extent, r_v), (line_x, r_v))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::TickText(self.name, TickLocator::U64Bits(d_v.to_bits())),
                MarkShape::Text(TextMark::new(
                    value.to_string(),
                    Default::default(),
                    12.0,
                    Point::new(label_x - 5.0, r_v + 8.0),
                )),
                Color::WHITE,
                None,
            ));
        }
        DrawableAxis::new(marks)
    }
}
