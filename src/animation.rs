use crate::animation::Animation::Focused;
use crate::animation::AnimationSegmentStatus::Ready;
use druid::kurbo::{Line, Point, Rect, Size};
use druid::piet::Color;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::num::NonZeroU32;
use std::ops::{Add, Deref, Index, IndexMut};
use InterpError::*;

pub struct AnimationCtxInner<'a> {
    focus: Option<AnimationSegmentId>,
    segments: &'a AnimationSegments,
}

impl<'a> AnimationCtxInner<'a> {
    fn new(focus: Option<AnimationSegmentId>, segments: &AnimationSegments) -> AnimationCtxInner {
        match focus {
            Some(current_segment) if !segments.contains(current_segment) => panic!(
                "animation segment out of range {:?} {:?}",
                current_segment, segments
            ),
            _ => AnimationCtxInner { focus, segments },
        }
    }
}

pub enum AnimationCtx<'a> {
    Full(AnimationCtxInner<'a>),
    Immediate(f64),
}

impl AnimationCtx<'_> {
    fn full(focus: Option<AnimationSegmentId>, segments: &AnimationSegments) -> AnimationCtx {
        AnimationCtx::Full(AnimationCtxInner::new(focus, segments))
    }

    fn on_focused_segment(&self) -> f64 {
        match self {
            AnimationCtx::Full(AnimationCtxInner { focus, segments }) => focus
                .and_then(|focus| segments.get(focus))
                .map_or(0., |seg| seg.translated),
            AnimationCtx::Immediate(eased) => *eased,
        }
    }

    pub fn current(&self) -> f64 {
        match self {
            AnimationCtx::Full(AnimationCtxInner { focus, segments }) => focus
                .and_then(|focus| segments.get(focus))
                .map_or(0., |seg| seg.translated),
            AnimationCtx::Immediate(eased) => *eased,
        }
    }

    pub fn clamped(&self) -> f64 {
        clamp_fraction(self.current())
    }

    pub fn with_segment<V>(
        &self,
        idx: AnimationSegmentId,
        mut f: impl FnMut(&AnimationCtx) -> V,
    ) -> Option<V> {
        match self {
            AnimationCtx::Full(AnimationCtxInner { segments, .. })
                if segments.get(idx).map_or(false, |s| s.status.is_active()) =>
            {
                Some(f(&Self::full(Some(idx), segments)))
            }
            _ => None,
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum InterpError {
    ValueMismatch,
    IndexOutOfBounds,
    Multiple,
    NotRunning,
}

pub(crate) type InterpResult = Result<(), InterpError>;
pub(crate) const OK: InterpResult = Ok(());

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InterpCoverage {
    Noop,
    Partial,
    Full,
}

impl Add for InterpCoverage {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        match (self, other) {
            (InterpCoverage::Full, InterpCoverage::Full) => InterpCoverage::Full,
            (InterpCoverage::Noop, InterpCoverage::Noop) => InterpCoverage::Noop,
            _ => InterpCoverage::Partial,
        }
    }
}

pub trait Interp: Default + Sized + Debug {
    type Value: HasInterp<Interp = Self>;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult;

    fn interp(&self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult;

    fn animation(self) -> Animation<Self::Value> {
        Animation::Focused(match self.coverage() {
            InterpCoverage::Noop => FocusedAnim::Noop,
            _ => FocusedAnim::Interp(self),
        })
    }

    fn coverage(&self) -> InterpCoverage;

    fn is_leaf() -> bool {
        false
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self;

    fn merge(self, other: Self) -> Self;

    fn build(start: Self::Value, end: Self::Value) -> Self;
}

pub trait HasInterp: Clone + Debug {
    type Interp: Interp<Value = Self>;
    fn tween(self, other: Self) -> Animation<Self> {
        Self::Interp::build(self, other).animation()
    }

    fn tween_now(self, other: Self, frac: f64) -> Result<Self, InterpError> {
        let mut res = self.clone();
        Self::Interp::build(self, other).interp(&AnimationCtx::Immediate(frac), &mut res)?;
        Ok(res)
    }
}

#[derive(Debug)]
pub enum SelectAnim<Value: HasInterp> {
    Single(AnimationSegmentId, Value::Interp),
    Many(Vec<(AnimationSegmentId, Value::Interp)>),
}

impl<Value: HasInterp> SelectAnim<Value> {
    fn prime(&mut self, val: &mut Value) -> InterpResult {
        match self {
            SelectAnim::Single(_, interp) => interp.prime(val),
            SelectAnim::Many(sels) => sels
                .iter_mut()
                .fold(OK, |r, (_, interp)| r.and_then(|_| interp.prime(val))), // TODO do this based on status
        }
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Value) -> InterpResult {
        match self {
            SelectAnim::Single(cur_idx, interp) => ctx
                .with_segment(*cur_idx, |ctx| interp.interp(ctx, val))
                .unwrap_or(OK),
            SelectAnim::Many(selects) => {
                for (idx, interp) in selects {
                    ctx.with_segment(*idx, |ctx| interp.interp(ctx, val))
                        .unwrap_or(OK)?; // TODO combine errors
                }
                OK
            }
        }
    }

    fn append(self, other: Self) -> Self {
        let mut vec = self.vec();
        vec.append(&mut other.vec());
        SelectAnim::Many(vec)
    }

    fn vec(self) -> Vec<(AnimationSegmentId, Value::Interp)> {
        match self {
            SelectAnim::Single(ai, interp) => vec![(ai, interp)],
            SelectAnim::Many(vec) => vec,
        }
    }

    fn select_internal(self) -> (Option<AnimationSegmentId>, Value::Interp) {
        match self {
            SelectAnim::Single(ai, interp) => (Some(ai), interp.select_animation_segment(ai)),
            SelectAnim::Many(vec) => vec
                .into_iter()
                .fold((None, Value::Interp::default()), |(_, cur), (ai, item)| {
                    (Some(ai), cur.merge(item))
                }),
        }
    }
}

pub trait CustomInterp<T> {
    fn interp(&self, ctx: &AnimationCtx, val: &mut T) -> InterpResult;
}

struct BasicInterp<T, F: Fn(f64) -> T>(F);

impl<T, F: Fn(f64) -> T> CustomInterp<T> for BasicInterp<T, F> {
    fn interp(&self, ctx: &AnimationCtx, val: &mut T) -> InterpResult {
        *val = self.0(ctx.current());
        OK
    }
}

impl<T: HasInterp, F: Fn(f64) -> T> From<F> for BasicInterp<T, F> {
    fn from(f: F) -> Self {
        BasicInterp(f)
    }
}

pub enum FocusedAnim<Value: HasInterp> {
    Noop,
    Interp(Value::Interp),
    Custom(Box<dyn CustomInterp<Value>>),
}

impl<Value: HasInterp> Debug for FocusedAnim<Value> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FocusedAnim::Noop => f.debug_struct("Noop").finish(),
            FocusedAnim::Interp(i) => f.debug_tuple("Interp").field(i).finish(),
            FocusedAnim::Custom(_) => f.debug_struct("Custom").finish(),
        }
    }
}

impl<Value: HasInterp> Default for FocusedAnim<Value> {
    fn default() -> Self {
        FocusedAnim::Noop
    }
}

impl<Value: HasInterp> FocusedAnim<Value> {
    fn coverage(&self) -> InterpCoverage {
        match self {
            FocusedAnim::Noop => InterpCoverage::Noop,
            FocusedAnim::Interp(interp) => interp.coverage(),
            FocusedAnim::Custom(_) => InterpCoverage::Partial, // Todo
        }
    }

    fn prime(&mut self, val: &mut Value) -> InterpResult {
        match self {
            FocusedAnim::Noop => OK,
            FocusedAnim::Interp(interp) => interp.prime(val),
            FocusedAnim::Custom(_) => OK, // Todo
        }
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Value) -> InterpResult {
        match self {
            FocusedAnim::Noop => OK,
            FocusedAnim::Interp(interp) => interp.interp(ctx, val),
            FocusedAnim::Custom(c) => c.interp(ctx, val),
        }
    }

    fn focus(&mut self) -> &mut Value::Interp {
        if !matches!(self, FocusedAnim::Interp(_)) {
            *self = FocusedAnim::Interp(Default::default())
        }

        match self {
            FocusedAnim::Interp(interp) => interp,
            _ => unreachable!("Just ensured we are expanded"),
        }
    }

    fn merge(self, other: FocusedAnim<Value>) -> FocusedAnim<Value> {
        match (self, other) {
            (FocusedAnim::Noop, other) => other,
            (other, FocusedAnim::Noop) => other,
            (FocusedAnim::Interp(i1), FocusedAnim::Interp(i2)) => FocusedAnim::Interp(i1.merge(i2)),
            (c @ FocusedAnim::Custom(_), other) => other,
            (_, c @ FocusedAnim::Custom(_)) => c,
        }
    }
}

#[derive(Debug)]
pub enum Animation<Value: HasInterp> {
    Focused(FocusedAnim<Value>),
    Select(SelectAnim<Value>, FocusedAnim<Value>),
}

impl<Value: HasInterp> Default for Animation<Value> {
    fn default() -> Self {
        Animation::Focused(FocusedAnim::Noop)
    }
}

impl<Value: HasInterp> Animation<Value> {
    pub fn coverage(&self) -> InterpCoverage {
        match self {
            Animation::Focused(f) => f.coverage(),
            Animation::Select(..) => InterpCoverage::Partial, // Always partial as some animations may not be running
        }
    }

    pub fn is_noop(&self) -> bool {
        matches!(self, Animation::Focused(FocusedAnim::Noop))
    }

    pub fn prime(&mut self, val: &mut Value) -> InterpResult {
        match self {
            Animation::Focused(foc) => foc.prime(val),
            Animation::Select(sa, other) => {
                sa.prime(val)?;
                other.prime(val)
            }
        }
    }

    pub fn using(&mut self, custom: impl CustomInterp<Value> + 'static) {
        match self {
            Animation::Focused(foc) => *foc = FocusedAnim::Custom(Box::new(custom)),
            Animation::Select(_, foc) => *foc = FocusedAnim::Custom(Box::new(custom)),
        }
    }

    // This could be a DerefMut, but Deref wouldn't work easily as it needs to expand Noops
    pub fn get(&mut self) -> &mut Value::Interp {
        match self {
            Animation::Focused(foc) => foc.focus(),
            Animation::Select(_, foc) => foc.focus(),
        }
    }

    pub fn interp(&self, ctx: &AnimationCtx, val: &mut Value) -> InterpResult {
        match self {
            Animation::Focused(foc) => foc.interp(ctx, val),
            Animation::Select(sa, foc) => {
                sa.interp(ctx, val)?;
                foc.interp(ctx, val)
            }
        }
    }

    pub fn select_anim(self, idx: AnimationSegmentId) -> Self {
        match self {
            Animation::Focused(FocusedAnim::Interp(interp)) => {
                Animation::Select(SelectAnim::Single(idx, interp), Default::default())
            }
            Animation::Select(sa, FocusedAnim::Interp(interp)) => Animation::Select(
                sa.append(SelectAnim::Single(idx, interp)),
                Default::default(),
            ),
            s => s,
        }
    }

    // TODO: fallible merge
    pub fn merge(self, other: Animation<Value>) -> Self {
        fn wrap_anim<V: HasInterp>(
            f: FocusedAnim<V>,
            idx: Option<AnimationSegmentId>,
        ) -> Animation<V> {
            match (f, idx) {
                (FocusedAnim::Interp(interp), Some(id)) if V::Interp::is_leaf() => {
                    Animation::Select(SelectAnim::Single(id, interp), FocusedAnim::Noop)
                }
                (f, _) => Animation::Focused(f),
            }
        }
        match (self, other) {
            (Animation::Focused(FocusedAnim::Noop), other) => other,
            (other, Animation::Focused(FocusedAnim::Noop)) => other,
            (Animation::Select(sa1, f1), Animation::Select(sa2, f2)) => {
                //let (_, si1) = sa1.select_internal();
                //let (a2, si2) = sa2.select_internal();
                //wrap_anim(si2.merge(si1), a2)

                //TODO: descending merge
                Animation::Select(sa1.append(sa2), f1.merge(f2))
            }
            (Animation::Select(sa, f1), Animation::Focused(f2)) => {
                let (a1, si1) = sa.select_internal();
                wrap_anim(FocusedAnim::Interp(si1).merge(f1.merge(f2)), a1)
            }
            (Animation::Focused(f1), Animation::Select(sa, f2)) => {
                let (a2, si2) = sa.select_internal();
                wrap_anim(f1.merge(f2).merge(FocusedAnim::Interp(si2)), a2)
            }
            (_, other) => other,
        }
    }
}

#[derive(Debug)]
pub struct MapInterp<Value: HasInterp, Key: Hash + Eq> {
    to_prime: Vec<(Key, Value)>,
    interps: Vec<(Key, Animation<Value>)>,
}

impl<Value: HasInterp, Key: Hash + Eq + Clone> MapInterp<Value, Key> {
    pub fn get(&mut self, key: &Key) -> &mut Animation<Value> {
        let idx = self
            .interps
            .iter()
            .position(|(k, v)| *k == *key)
            .unwrap_or_else(|| {
                let idx = self.interps.len();
                self.interps.push((key.clone(), Default::default()));
                idx
            });
        &mut self.interps[idx].1
    }
}

impl<Value: HasInterp, Key: Hash + Eq> Default for MapInterp<Value, Key> {
    fn default() -> Self {
        MapInterp {
            to_prime: Default::default(),
            interps: Default::default(),
        }
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

pub trait EnterExit {
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

    fn interp(&self, ctx: &AnimationCtx, val: &mut HashMap<Key, Value>) -> InterpResult {
        let mut loop_err: Option<InterpError> = None;
        for (key, interp) in self.interps.iter() {
            let cur_err = val
                .get_mut(key)
                .map(|value| interp.interp(ctx, value))
                .unwrap_or(Err(IndexOutOfBounds));

            match (&mut loop_err, cur_err) {
                (loop_err @ None, Err(c_e)) => *loop_err = Some(c_e),
                (Some(l_e), Err(c_e)) if *l_e != c_e => *l_e = InterpError::Multiple,
                _ => (),
            }
        }
        loop_err.map(Err).unwrap_or(OK)
    }

    fn coverage(&self) -> InterpCoverage {
        if self.interps.is_empty() {
            InterpCoverage::Noop
        } else {
            InterpCoverage::Partial
        }
    }

    fn select_animation_segment(self, a_idx: AnimationSegmentId) -> Self {
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
            to_prime,
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
                    interps.push((k, o.clone().tween(n)));
                }
                (None, Some(n)) => {
                    let e = n.enter();
                    interps.push((k.clone(), e.clone().tween(n)));
                    to_prime.push((k, e));
                }
                (Some(o), None) => {
                    let e = o.exit();
                    interps.push((k, o.clone().tween(e)));
                }
                _ => (),
            }
        }

        MapInterp { to_prime, interps }
    }
}

#[derive(Debug, Default)]
pub struct F64Interp {
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

    fn interp(&self, ctx: &AnimationCtx, val: &mut f64) -> InterpResult {
        *val = Self::interp_raw(self.start, self.end, ctx.current());
        OK
    }

    fn coverage(&self) -> InterpCoverage {
        if self.start == self.end {
            InterpCoverage::Noop
        } else {
            InterpCoverage::Full
        }
    }

    fn is_leaf() -> bool {
        true
    }

    fn select_animation_segment(self, _idx: AnimationSegmentId) -> Self {
        self //TODO: Need better protocol around this
    }

    fn merge(self, other: Self) -> Self {
        // Would it make sense to make use of the other point, and compose the
        // interpolations? Seems niche
        Self {
            start: self.start,
            end: other.end,
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Default)]
pub struct PointInterp {
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

    fn interp(&self, ctx: &AnimationCtx, val: &mut Point) -> InterpResult {
        self.x.interp(ctx, &mut val.x)?;
        self.y.interp(ctx, &mut val.y)
    }

    fn coverage(&self) -> InterpCoverage {
        self.x.coverage() + self.y.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
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
pub struct SizeInterp {
    width: Animation<f64>,
    height: Animation<f64>,
}

impl HasInterp for Size {
    type Interp = SizeInterp;
}

impl Interp for SizeInterp {
    type Value = Size;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        self.width.prime(&mut val.width)?;
        self.height.prime(&mut val.height)
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.width.interp(ctx, &mut val.width)?;
        self.height.interp(ctx, &mut val.height)
    }

    fn coverage(&self) -> InterpCoverage {
        self.width.coverage() + self.height.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        Self {
            width: self.width.select_anim(idx),
            height: self.height.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            width: self.width.merge(other.width),
            height: self.height.merge(other.height),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self {
            width: start.width.tween(end.width),
            height: start.height.tween(end.height),
        }
    }
}

#[derive(Debug, Default)]
pub struct LineInterp {
    p0: Animation<Point>,
    p1: Animation<Point>,
}

impl HasInterp for Line {
    type Interp = LineInterp;
}

impl Interp for LineInterp {
    type Value = Line;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        self.p0.prime(&mut val.p0)?;
        self.p1.prime(&mut val.p1)
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.p0.interp(ctx, &mut val.p0)?;
        self.p1.interp(ctx, &mut val.p1)
    }

    fn coverage(&self) -> InterpCoverage {
        self.p0.coverage() + self.p1.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        Self {
            p0: self.p0.select_anim(idx),
            p1: self.p1.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            p0: self.p0.merge(other.p0),
            p1: self.p1.merge(other.p1),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self {
            p0: start.p0.tween(end.p0),
            p1: start.p1.tween(end.p1),
        }
    }
}

#[derive(Debug)]
pub enum RectInterp {
    Coords {
        x0: Animation<f64>,
        y0: Animation<f64>,
        x1: Animation<f64>,
        y1: Animation<f64>,
    },
    OriginSize {
        origin: Animation<Point>,
        size: Animation<Size>,
    },
    Points {
        origin: Animation<Point>,
        far: Animation<Point>,
    },
}

impl HasInterp for Rect {
    type Interp = RectInterp;
}

impl Default for RectInterp {
    fn default() -> Self {
        RectInterp::Coords {
            x0: Default::default(),
            y0: Default::default(),
            x1: Default::default(),
            y1: Default::default(),
        }
    }
}

impl Interp for RectInterp {
    type Value = Rect;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        match self {
            RectInterp::Coords { x0, y0, x1, y1 } => {
                x0.prime(&mut val.x1)?;
                y0.prime(&mut val.y1)?;
                x1.prime(&mut val.x1)?;
                y1.prime(&mut val.y1)
            }
            _ => OK, // Synthesized so priming won't work
        }
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        match self {
            RectInterp::Coords { x0, y0, x1, y1 } => {
                x0.interp(ctx, &mut val.x0)?;
                y0.interp(ctx, &mut val.y0)?;
                x1.interp(ctx, &mut val.x1)?;
                y1.interp(ctx, &mut val.y1)
            }
            RectInterp::OriginSize { origin, size } => {
                let (mut o, mut s) = (val.origin(), val.size());
                origin.interp(ctx, &mut o)?;
                size.interp(ctx, &mut s)?;
                *val = Rect::from_origin_size(o, s);
                OK
            }
            RectInterp::Points { origin, far } => {
                let (mut o, mut f) = (val.origin(), Point::new(val.x1, val.y1));
                origin.interp(ctx, &mut o)?;
                far.interp(ctx, &mut f)?;
                *val = Rect::from_points(o, f);
                OK
            }
        }
    }

    fn coverage(&self) -> InterpCoverage {
        match self {
            RectInterp::Coords { x0, y0, x1, y1 } => {
                x0.coverage() + y0.coverage() + x1.coverage() + y1.coverage()
            }
            RectInterp::OriginSize { origin, size } => origin.coverage() + size.coverage(),
            RectInterp::Points { origin, far } => origin.coverage() + far.coverage(),
        }
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        match self {
            RectInterp::Coords { x0, y0, x1, y1 } => RectInterp::Coords {
                x0: x0.select_anim(idx),
                y0: y0.select_anim(idx),
                x1: x1.select_anim(idx),
                y1: y1.select_anim(idx),
            },
            RectInterp::OriginSize { origin, size } => RectInterp::OriginSize {
                origin: origin.select_anim(idx),
                size: size.select_anim(idx),
            },
            RectInterp::Points { origin, far } => RectInterp::Points {
                origin: origin.select_anim(idx),
                far: far.select_anim(idx),
            },
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (
                RectInterp::Coords { x0, y0, x1, y1 },
                RectInterp::Coords {
                    x0: x0_1,
                    y0: y0_1,
                    x1: x1_1,
                    y1: y1_1,
                },
            ) => RectInterp::Coords {
                x0: x0.merge(x0_1),
                y0: y0.merge(y0_1),
                x1: x1.merge(x1_1),
                y1: y1.merge(y1_1),
            },
            (
                RectInterp::OriginSize { origin, size },
                RectInterp::OriginSize {
                    origin: origin_1,
                    size: size_1,
                },
            ) => RectInterp::OriginSize {
                origin: origin.merge(origin_1),
                size: size.merge(size_1),
            },
            (
                RectInterp::Points { origin, far },
                RectInterp::Points {
                    origin: origin_1,
                    far: far_1,
                },
            ) => RectInterp::Points {
                origin: origin.merge(origin_1),
                far: far.merge(far_1),
            },
            (_, other) => other,
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        RectInterp::Coords {
            x0: start.x0.tween(end.x0),
            y0: start.y0.tween(end.y0),
            x1: start.x1.tween(end.x1),
            y1: start.y1.tween(end.y1),
        }
    }
}

#[derive(Debug, Default)]
pub struct StringInterp {
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

    fn interp(&self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        // TODO: do this by modifying? Can't assume that calls are in order though
        let step = ((self.steps as f64) * ctx.clamped()) as isize;
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

    fn coverage(&self) -> InterpCoverage {
        if self.steps == 0 {
            InterpCoverage::Noop
        } else {
            InterpCoverage::Full
        }
    }

    fn is_leaf() -> bool {
        true
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        self
    }

    fn merge(self, other: Self) -> Self {
        other
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self::new(&start, &end)
    }
}

#[derive(Debug)]
pub enum ColorInterp {
    Rgba(
        Animation<f64>,
        Animation<f64>,
        Animation<f64>,
        Animation<f64>,
    ),
    Noop,
}

impl Default for ColorInterp {
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
        // Synthesizing values each time
        OK
    }

    fn interp(&self, ctx: &AnimationCtx, val: &mut Color) -> InterpResult {
        match self {
            ColorInterp::Rgba(ri, gi, bi, ai) => {
                let (mut r, mut g, mut b, mut a) = val.as_rgba();
                ri.interp(ctx, &mut r)?;
                gi.interp(ctx, &mut g)?;
                bi.interp(ctx, &mut b)?;
                ai.interp(ctx, &mut a)?;

                *val = Color::rgba(r, g, b, a);
                OK
            }
            ColorInterp::Noop => OK,
        }
    }

    fn coverage(&self) -> InterpCoverage {
        match self {
            ColorInterp::Rgba(r, g, b, a) => {
                r.coverage() + g.coverage() + b.coverage() + a.coverage()
            }
            ColorInterp::Noop => InterpCoverage::Noop,
        }
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        match self {
            ColorInterp::Rgba(r, g, b, a) => ColorInterp::Rgba(
                r.select_anim(idx),
                g.select_anim(idx),
                b.select_anim(idx),
                a.select_anim(idx),
            ),
            ColorInterp::Noop => self,
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (ColorInterp::Rgba(r, g, b, a), ColorInterp::Rgba(r1, g1, b1, a1)) => {
                ColorInterp::Rgba(r.merge(r1), g.merge(g1), b.merge(b1), a.merge(a1))
            }
            (ColorInterp::Noop, other) => other,
            (s, ColorInterp::Noop) => s,
        }
    }

    fn build(old: Color, new: Color) -> ColorInterp {
        let (r, g, b, a) = old.as_rgba();
        let (r2, g2, b2, a2) = new.as_rgba();

        ColorInterp::Rgba(
            F64Interp::build(r, r2).animation(),
            F64Interp::build(g, g2).animation(),
            F64Interp::build(b, b2).animation(),
            F64Interp::build(a, a2).animation(),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ReadyStatus {
    Waiting,
    Running,
}

#[derive(Clone, Debug, PartialEq)]
enum AnimationSegmentStatus {
    Pending(f64),            // delay after ready
    Ready(f64, ReadyStatus), // start time
    Completing,
}

impl AnimationSegmentStatus {
    fn is_active(&self) -> bool {
        match self {
            AnimationSegmentStatus::Ready(_, ReadyStatus::Running)
            | AnimationSegmentStatus::Completing => true,
            _ => false,
        }
    }
}

pub enum CustomAnimationCurve {
    Function(fn(f64) -> f64),
    Boxed(Box<dyn FnMut(f64) -> f64>),
}

impl CustomAnimationCurve {
    fn translate(&mut self, t: f64) -> f64 {
        match self {
            CustomAnimationCurve::Function(f) => f(t),
            CustomAnimationCurve::Boxed(f) => f(t),
        }
    }
}

fn clamp_fraction(f: f64) -> f64 {
    f.max(0.).min(1.)
}

impl Debug for CustomAnimationCurve {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CustomAnimationCurve::Function(f) => formatter
                .debug_struct("CustomAnimationCurve::Function")
                .field("f", f)
                .finish(),
            CustomAnimationCurve::Boxed(_) => formatter
                .debug_struct("CustomAnimationCurve::Closure")
                .finish(),
        }
    }
}

impl From<fn(f64) -> f64> for AnimationCurve {
    fn from(f: fn(f64) -> f64) -> Self {
        AnimationCurve::Custom(CustomAnimationCurve::Function(f))
    }
}

#[derive(Debug)]
pub enum AnimationCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    OutElastic,
    OutBounce,
    OutSine,
    //    CubicBezier(CubicBezierAnimationCurve),
    //    Spring(SpringAnimationCurve),
    Custom(CustomAnimationCurve),
}

impl AnimationCurve {
    fn translate(&mut self, t: f64) -> f64 {
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
                } else if t > 0.999 {
                    1.
                } else {
                    2.0f64.powf(-10.0 * t) * ((t - s) * (2.0 * PI) / p).sin() + 1.0
                }
            }
            AnimationCurve::OutSine => (t * PI * 0.5).sin(),
            AnimationCurve::OutBounce => {
                if t < (1. / 2.75) {
                    7.5625 * t * t
                } else if t < (2. / 2.75) {
                    let t = t - (1.5 / 2.75);
                    7.5625 * t * t + 0.75
                } else if t < (2.5 / 2.75) {
                    let t = t - (2.25 / 2.75);
                    7.5625 * t * t + 0.9375
                } else {
                    let t = t - (2.625 / 2.75);
                    7.5625 * t * t + 0.984375
                }
            }
            AnimationCurve::Custom(c) => c.translate(t),
        }
    }
}

#[derive(Debug)]
struct AnimationSegment {
    dur_nanos: f64,
    curve: AnimationCurve,
    status: AnimationSegmentStatus,
    since_start: f64,
    fraction: f64,
    translated: f64,
}

impl AnimationSegment {
    pub fn new(dur_nanos: f64, curve: AnimationCurve, status: AnimationSegmentStatus) -> Self {
        AnimationSegment {
            dur_nanos,
            curve,
            status,
            since_start: 0.,
            fraction: 0.,
            translated: 0.,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub enum AnimationEvent {
    Named(&'static str),
    SegmentEnded(AnimationSegmentId),
}

type ASOffset = u32;
type ASVersion = NonZeroU32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct AnimationSegmentId {
    offset: ASOffset,
    version: ASVersion,
}

impl AnimationSegmentId {
    pub fn new(offset: ASOffset, version: NonZeroU32) -> Self {
        AnimationSegmentId { offset, version }
    }
}

#[derive(Debug)]
enum ASEntry {
    Busy(ASVersion, AnimationSegment),
    Free(ASVersion, ASOffset), // next free
    LastFree(ASVersion),
}

// SlotMap?
#[derive(Default, Debug)]
struct AnimationSegments {
    contents: Vec<ASEntry>,
    size: ASOffset,
    first_free: Option<ASOffset>,
}

impl AnimationSegments {
    fn iter(&self) -> impl Iterator<Item = &AnimationSegment> {
        self.contents.iter().flat_map(|content| match content {
            ASEntry::Busy(_, seg) => Some(seg),
            _ => None,
        })
    }

    fn segment_loop(
        &mut self,
        mut f: impl FnMut(AnimationSegmentId, &mut AnimationSegment) -> bool,
    ) {
        let mut removals = false;
        for (offset, entry) in self.contents.iter_mut().enumerate() {
            let offset = offset as ASOffset;
            let (version, remove) = match entry {
                ASEntry::Busy(version, segment) => (
                    version.clone(),
                    f(AnimationSegmentId::new(offset, *version), segment),
                ),
                ASEntry::Free(version, _) | ASEntry::LastFree(version) => (*version, false),
            };

            if remove {
                removals = true;
                *entry = self
                    .first_free
                    .map(|next_free| ASEntry::Free(version, next_free))
                    .unwrap_or_else(|| ASEntry::LastFree(version));
                self.first_free = Some(offset);
                self.size -= 1;
                log::info!("Removed {:?} {:?}", offset, entry);
            }
        }
        if removals {
            log::info!(
                "size {} capacity {} und_capacity {}",
                self.size,
                self.contents.len(),
                self.contents.capacity()
            );
        }
    }

    fn is_empty(&self) -> bool {
        self.size == 0
    }

    fn insert(&mut self, segment: AnimationSegment) -> AnimationSegmentId {
        self.size += 1;
        if let Some(offset) = self.first_free.take() {
            let entry = &mut self.contents[offset as usize];
            let (first_free, version) = match entry {
                ASEntry::LastFree(version) => (None, version),
                ASEntry::Free(version, next_free) => (Some(*next_free), version),
                ASEntry::Busy(..) => panic!("Free list pointing to busy entry"),
            };
            self.first_free = first_free;
            let version = NonZeroU32::new(version.get() + 1).unwrap();
            *entry = ASEntry::Busy(version, segment);
            AnimationSegmentId::new(offset, version)
        } else {
            let version = NonZeroU32::new(1).unwrap();
            let id = AnimationSegmentId::new(self.contents.len() as u32, version);
            self.contents.push(ASEntry::Busy(version, segment));
            id
        }
    }

    fn contains(&self, id: AnimationSegmentId) -> bool {
        id.offset < self.contents.len() as u32
            && matches!(self.contents[id.offset as usize], ASEntry::Busy(version, _) if version == id.version)
    }

    fn get(&self, id: AnimationSegmentId) -> Option<&AnimationSegment> {
        self.contents
            .get(id.offset as usize)
            .and_then(|entry| match entry {
                ASEntry::Busy(version, seg) if *version == id.version => Some(seg),
                _ => None,
            })
    }

    fn get_mut(&mut self, id: AnimationSegmentId) -> Option<&mut AnimationSegment> {
        self.contents
            .get_mut(id.offset as usize)
            .and_then(|entry| match entry {
                ASEntry::Busy(version, seg) if *version == id.version => Some(seg),
                _ => None,
            })
    }

    fn clear(&mut self) {
        self.contents.clear();
        self.size = Default::default();
        self.first_free = Default::default();
    }
}

#[derive(Default, Debug)]
pub struct Animator<T: HasInterp> {
    cur_nanos: f64,
    pending_events: VecDeque<AnimationEvent>,
    pending_starts: HashMap<AnimationEvent, Vec<AnimationSegmentId>>,
    segments: AnimationSegments,
    pub animation: Animation<T>,
}

impl<T: HasInterp> Animator<T> {
    pub fn advance(&mut self, nanos: f64, current: &mut T) -> InterpResult {
        if self.segments.is_empty() {
            return InterpResult::Err(InterpError::NotRunning);
        }
        self.cur_nanos += nanos;
        let cur_nanos = self.cur_nanos;

        let segments = &mut self.segments;

        let pending_events = &mut self.pending_events;
        segments.segment_loop(|id, segment| {
            match segment.status.clone() {
                AnimationSegmentStatus::Ready(start, ready) => {
                    segment.since_start = cur_nanos - start;

                    let is_running = match ready {
                        ReadyStatus::Waiting if segment.since_start > 0. => {
                            segment.status =
                                AnimationSegmentStatus::Ready(start, ReadyStatus::Running);
                            true
                        }
                        ReadyStatus::Running => true,
                        _ => false,
                    };

                    if is_running {
                        let fraction = segment.since_start / segment.dur_nanos;
                        if fraction <= 1.0 {
                            segment.fraction = fraction;
                            segment.translated = segment.curve.translate(fraction);
                        } else {
                            // This segment will go through one more cycle to give interps
                            // a chance to recover from any discontinuous curves
                            segment.fraction = 1.0;
                            segment.translated = 1.0;
                            segment.status = AnimationSegmentStatus::Completing;
                            pending_events.push_back(AnimationEvent::SegmentEnded(id));
                        }
                    }
                    false
                }
                AnimationSegmentStatus::Completing => true,
                AnimationSegmentStatus::Pending(_) => false,
            }
        });

        let ctx = AnimationCtx::full(None, &segments);
        let res = self.animation.interp(&ctx, current);

        for event in pending_events.drain(..) {
            Self::event_impl(cur_nanos, &mut self.pending_starts, segments, event)
        }

        if self.segments.is_empty() {
            self.animation = Default::default();
        }
        res
    }

    pub fn event(&mut self, event: AnimationEvent) {
        Self::event_impl(
            self.cur_nanos,
            &mut self.pending_starts,
            &mut self.segments,
            event,
        );
    }

    fn event_impl(
        cur_nanos: f64,
        pending_starts: &mut HashMap<AnimationEvent, Vec<AnimationSegmentId>>,
        segments: &mut AnimationSegments,
        event: AnimationEvent,
    ) {
        // TODO: with repeating segments do not remove?
        // Re triggering?
        if let Some(ids) = pending_starts.remove(&event) {
            for id in ids {
                if let Some(seg) = segments.get_mut(id) {
                    if let AnimationSegmentStatus::Pending(delay) = seg.status.clone() {
                        seg.status =
                            AnimationSegmentStatus::Ready(cur_nanos + delay, ReadyStatus::Waiting);
                    }
                }
            }
        }
    }

    pub fn running(&self) -> bool {
        // TODO: If we had waiting ones we could return a minimum time until one had to start
        // then use a timer to get it
        !self
            .segments
            .iter()
            .all(|s| matches!(s.status, AnimationSegmentStatus::Pending(_)))
    }

    pub fn add_animation_segment(
        &mut self,
        delay_nanos: u64,
        dur_nanos: u64,
        curve: impl Into<AnimationCurve>,
        after: Option<AnimationEvent>,
    ) -> AnimationSegmentId {
        let delay_nanos = delay_nanos as f64;
        let start = self.cur_nanos + delay_nanos;
        let status = if after.is_some() {
            AnimationSegmentStatus::Pending(delay_nanos)
        } else if delay_nanos > 0. {
            AnimationSegmentStatus::Ready(start, ReadyStatus::Waiting)
        } else {
            AnimationSegmentStatus::Ready(start, ReadyStatus::Running)
        };
        let anim_id = self.segments.insert(AnimationSegment::new(
            dur_nanos.max(1) as f64,
            curve.into(),
            status,
        ));
        if let Some(after) = after {
            self.pending_starts
                .entry(after)
                .or_insert_with(|| vec![])
                .push(anim_id);
        }
        return anim_id;
    }

    pub fn add_animation(
        &mut self,
        interp: Animation<T>,
        from_now_nanos: u64,
        dur_nanos: u64,
        curve: impl Into<AnimationCurve>,
        after: Option<AnimationEvent>,
        current: &mut T,
    ) -> Result<(), InterpError> {
        let mut interp = interp;
        interp.prime(current).and_then(|()| {
            let anim_idx = self.add_animation_segment(from_now_nanos, dur_nanos, curve, after);
            let taken = std::mem::take(&mut self.animation);
            self.animation = taken.merge(interp.select_anim(anim_idx));
            OK
        })
    }
}
