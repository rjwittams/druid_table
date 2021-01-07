use druid_widget_nursery::animation::{AnimationCtx, AnimationId, AnimationStatus};
use druid::kurbo::{Line, Point, Rect, Size};
use druid::piet::Color;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::ops::Add;
use InterpError::*;

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

pub trait Interp: Default + Debug {
    type Value: HasInterp<Interp = Self>;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult;

    fn animation(self) -> InterpNode<Self::Value> {
        InterpNode {
            focused: (match self.coverage() {
                InterpCoverage::Noop => None,
                _ => Some(InterpHolder::Interp(self)),
            }),
            selected: Default::default(),
        }
    }

    fn coverage(&self) -> InterpCoverage;

    fn is_leaf() -> bool {
        false
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self>;

    fn merge(self, other: Self) -> Self;

    fn build(start: Self::Value, end: Self::Value) -> Self;
}

pub trait HasInterp: Clone + Debug {
    type Interp: Interp<Value = Self>;

    fn tween_ref(&self, other: &Self) -> InterpNode<Self> {
        self.clone().tween(other.clone())
    }

    fn tween(self, other: Self) -> InterpNode<Self> {
        Self::Interp::build(self, other).animation()
    }

    fn tween_now(self, other: Self, frac: f64) -> Result<Self, InterpError> {
        let mut res = self.clone();
        Self::Interp::build(self, other).interp(
            &AnimationCtx::running(frac),
            &mut res,
        )?;
        Ok(res)
    }
}

pub trait CustomInterp<T> {
    fn interp(&self, ctx: &AnimationCtx, val: &mut T) -> InterpResult;
}

struct BasicInterp<T, F: Fn(f64) -> T>(F);

impl<T, F: Fn(f64) -> T> CustomInterp<T> for BasicInterp<T, F> {
    fn interp(&self, ctx: &AnimationCtx, val: &mut T) -> InterpResult {
        *val = self.0(ctx.progress());
        OK
    }
}

impl<T: HasInterp, F: Fn(f64) -> T> From<F> for BasicInterp<T, F> {
    fn from(f: F) -> Self {
        BasicInterp(f)
    }
}

#[derive(Debug)]
pub enum InterpHolder<Value: HasInterp> {
    Interp(Value::Interp),
    Custom(Box<dyn CustomInterp<Value>>),
}

impl<Value: HasInterp> Debug for Box<dyn CustomInterp<Value>> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("custom interp")
    }
}

// Todo check if needed
impl<Value: HasInterp> Default for InterpHolder<Value> {
    fn default() -> Self {
        InterpHolder::Interp(Default::default())
    }
}

impl<Value: HasInterp> InterpHolder<Value> {
    fn coverage(&self) -> InterpCoverage {
        match self {
            InterpHolder::Interp(interp) => interp.coverage(),
            InterpHolder::Custom(_) => InterpCoverage::Partial, // Todo
        }
    }

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Value) -> InterpResult {
        match self {
            InterpHolder::Interp(interp) => interp.interp(ctx, val),
            InterpHolder::Custom(c) => c.interp(ctx, val),
        }
    }

    //TODO: Fallible merge
    fn merge(self, other: InterpHolder<Value>) -> InterpHolder<Value> {
        match (self, other) {
            (InterpHolder::Interp(i1), InterpHolder::Interp(i2)) => {
                InterpHolder::Interp(i1.merge(i2))
            }
            (_c @ InterpHolder::Custom(_), other) => other,
            (_, c @ InterpHolder::Custom(_)) => c,
        }
    }

    fn push_down_anim(self, id: AnimationId) -> Result<Self, Self> {
        match self {
            InterpHolder::Interp(interp) => interp
                .select_animation_segment(id)
                .map(InterpHolder::Interp)
                .map_err(InterpHolder::Interp),
            InterpHolder::Custom(c) => Err(InterpHolder::Custom(c)), // TOD
        }
    }
}

#[derive(Debug)]
pub struct InterpNode<Value: HasInterp> {
    selected: Vec<(AnimationId, InterpHolder<Value>)>,
    focused: Option<InterpHolder<Value>>,
}

impl<Value: HasInterp> Default for InterpNode<Value> {
    fn default() -> Self {
        Self {
            selected: Default::default(),
            focused: Default::default(),
        }
    }
}

fn select_internal<Value: HasInterp>(
    by_id: Vec<(AnimationId, InterpHolder<Value>)>,
) -> (
    Option<InterpHolder<Value>>,
    Vec<(AnimationId, InterpHolder<Value>)>,
) {
    let mut merged: Option<InterpHolder<Value>> = Default::default();
    let mut remaining: Vec<_> = Default::default();

    for (ai, item) in by_id.into_iter() {
        match item.push_down_anim(ai) {
            Ok(to_merge) => {
                merged = if merged.is_some() {
                    merged.map(|m| m.merge(to_merge))
                } else {
                    Some(to_merge)
                }
            }
            Err(leave_as_is) => {
                remaining.push((ai, leave_as_is));
            }
        }
    }
    (merged, remaining)
}

impl<Value: HasInterp> InterpNode<Value> {
    pub fn coverage(&self) -> InterpCoverage {
        if self.selected.is_empty() {
            self.focused
                .as_ref()
                .map_or(InterpCoverage::Noop, |f| f.coverage())
        } else {
            InterpCoverage::Partial // Always partial as some animations may not be running
        }
    }

    pub fn is_noop(&self) -> bool {
        self.selected.is_empty() && self.focused.is_none()
    }

    pub fn use_custom(&mut self, custom: impl CustomInterp<Value> + 'static) {
        self.focused = Some(InterpHolder::Custom(Box::new(custom)));
    }

    // This could be a DerefMut, but Deref wouldn't work easily as it needs to expand Noops
    pub fn get(&mut self) -> &mut Value::Interp {
        if !matches!(self.focused, Some(InterpHolder::Interp(_))) {
            self.focused = Some(InterpHolder::Interp(Default::default()))
        }

        match &mut self.focused {
            Some(InterpHolder::Interp(interp)) => interp,
            _ => unreachable!("Just ensured we are expanded"),
        }
    }

    pub fn interp(&mut self, ctx: &AnimationCtx, val: &mut Value) -> InterpResult {
        let mut first = true;
        for (idx, interp) in &mut self.selected {
            ctx.with_animation_full(*idx, false, |ctx| {
                if ctx.status() != AnimationStatus::NotRunning {
                    first = false;
                    interp.interp(ctx, val)
                } else {
                    OK
                }
            })
            .unwrap_or(OK); // TODO combine errors
        }

        if let Some(f) = &mut self.focused {
            f.interp(ctx, val)
        } else {
            OK
        }
    }

    pub fn select_anim(self, id: AnimationId) -> Self {
        match self {
            InterpNode {
                mut selected,
                focused: Some(interp),
            } => {
                selected.push((id, interp));
                InterpNode {
                    selected,
                    focused: Default::default(),
                }
            }
            s => s, // TODO custom
        }
    }

    // TODO: fallible merge
    pub fn merge(self, other: InterpNode<Value>) -> Self {
        //let mut start = format!("merging\n\t A:{:?}\n\t B:{:?}\n\t", self, other);

        if self.coverage() == InterpCoverage::Noop {
            other
        } else if other.coverage() == InterpCoverage::Noop {
            self
        } else {
            let (si1, r1) = select_internal(self.selected);
            let (si2, r2) = select_internal(other.selected);
            let selected = r1.into_iter().chain(r2.into_iter()).collect();

            let all = si1
                .into_iter()
                .chain(self.focused.into_iter())
                .chain(si2.into_iter())
                .chain(other.focused.into_iter());

            let focused: Option<InterpHolder<Value>> = all.fold(None, |res, to_add| {
                if res.is_none() {
                    Some(to_add)
                } else {
                    res.map(|r: InterpHolder<Value>| r.merge(to_add))
                }
            });

            let ret = InterpNode { selected, focused };

            ret
        }
    }
}

pub trait EnterExit {
    fn enter(&self) -> Self;
    fn exit(&self) -> Self;
}

#[derive(Debug)]
pub struct MapInterp<Value: HasInterp, Key> {
    to_enlist: Vec<(Key, Value)>,
    to_retire: Vec<Key>,
    interps: Vec<(Key, InterpNode<Value>)>,
}

impl<Value: HasInterp, Key: Hash + Eq + Clone> MapInterp<Value, Key> {
    pub fn get(&mut self, key: &Key) -> &mut InterpNode<Value> {
        let idx = self
            .interps
            .iter()
            .position(|(k, _)| *k == *key)
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
            to_enlist: Default::default(),
            to_retire: Default::default(),
            interps: Default::default(),
        }
    }
}

impl<Value: HasInterp + EnterExit + Debug, Key: Eq + Hash + Clone + Debug> HasInterp
    for HashMap<Key, Value>
{
    type Interp = MapInterp<Value, Key>;
}

impl<Value: HasInterp + EnterExit + Debug, Key: Debug + Hash + Eq + Clone> Interp
    for MapInterp<Value, Key>
{
    type Value = HashMap<Key, Value>;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut HashMap<Key, Value>) -> InterpResult {
        if !self.to_enlist.is_empty() {
            for (k, v) in self.to_enlist.drain(..) {
                val.insert(k, v);
            }
        }
        let mut loop_err: Option<InterpError> = None;
        for (key, interp) in self.interps.iter_mut() {
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

    fn select_animation_segment(self, a_idx: AnimationId) -> Result<Self, Self> {
        Ok(MapInterp {
            to_enlist: self.to_enlist,
            to_retire: self.to_retire,
            interps: self
                .interps
                .into_iter()
                .map(|(key, interp)| (key, interp.select_anim(a_idx)))
                .collect(),
        })
    }

    fn merge(self, other: MapInterp<Value, Key>) -> Self {
        let mut interps: HashMap<_, _> = self.interps.into_iter().collect();
        for (key, interp) in other.interps.into_iter() {
            let new_interp = if let Some(cur) = interps.remove(&key) {
                cur.merge(interp)
            } else {
                interp
            };
            if new_interp.coverage() != InterpCoverage::Noop {
                interps.insert(key, new_interp);
            }
        }

        MapInterp {
            to_enlist: self
                .to_enlist
                .into_iter()
                .chain(other.to_enlist.into_iter())
                .collect(),
            to_retire: self
                .to_retire
                .into_iter()
                .chain(other.to_retire.into_iter())
                .collect(),
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
        let mut to_enlist = Vec::new();
        let mut to_retire = Vec::new();

        for (k, v) in matched_marks.into_iter() {
            match v {
                (Some(o), Some(n)) => {
                    interps.push((k, o.clone().tween(n)));
                }
                (None, Some(n)) => {
                    let e = n.enter();
                    interps.push((k.clone(), e.clone().tween(n)));
                    to_enlist.push((k, e));
                }
                (Some(o), None) => {
                    let e = o.exit();
                    interps.push((k.clone(), o.clone().tween(e)));
                    to_retire.push(k);
                }
                _ => (),
            }
        }

        MapInterp {
            to_enlist,
            to_retire,
            interps,
        }
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

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut f64) -> InterpResult {
        let raw = Self::interp_raw(self.start, self.end, ctx.progress());
        if ctx.additive() {
            let diff = raw - self.start;
            *val += diff;
        } else {
            *val = raw
        }
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

    fn select_animation_segment(self, _idx: AnimationId) -> Result<Self, Self> {
        Err(self)
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
    pub x: InterpNode<f64>,
    pub y: InterpNode<f64>,
}

impl HasInterp for Point {
    type Interp = PointInterp;
}

impl Interp for PointInterp {
    type Value = Point;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Point) -> InterpResult {
        self.x.interp(ctx, &mut val.x)?;
        self.y.interp(ctx, &mut val.y)
    }

    fn coverage(&self) -> InterpCoverage {
        self.x.coverage() + self.y.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            x: self.x.select_anim(idx),
            y: self.y.select_anim(idx),
        })
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
    width: InterpNode<f64>,
    height: InterpNode<f64>,
}

impl HasInterp for Size {
    type Interp = SizeInterp;
}

impl Interp for SizeInterp {
    type Value = Size;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.width.interp(ctx, &mut val.width)?;
        self.height.interp(ctx, &mut val.height)
    }

    fn coverage(&self) -> InterpCoverage {
        self.width.coverage() + self.height.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            width: self.width.select_anim(idx),
            height: self.height.select_anim(idx),
        })
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
    pub p0: InterpNode<Point>,
    pub p1: InterpNode<Point>,
}

impl HasInterp for Line {
    type Interp = LineInterp;
}

impl Interp for LineInterp {
    type Value = Line;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.p0.interp(ctx, &mut val.p0)?;
        self.p1.interp(ctx, &mut val.p1)
    }

    fn coverage(&self) -> InterpCoverage {
        self.p0.coverage() + self.p1.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            p0: self.p0.select_anim(idx),
            p1: self.p1.select_anim(idx),
        })
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
        x0: InterpNode<f64>,
        y0: InterpNode<f64>,
        x1: InterpNode<f64>,
        y1: InterpNode<f64>,
    },
    OriginSize {
        origin: InterpNode<Point>,
        size: InterpNode<Size>,
    },
    Points {
        origin: InterpNode<Point>,
        far: InterpNode<Point>,
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

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
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

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(match self {
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
        })
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

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
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

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Err(self)
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
        InterpNode<f64>,
        InterpNode<f64>,
        InterpNode<f64>,
        InterpNode<f64>,
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

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Color) -> InterpResult {
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

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(match self {
            ColorInterp::Rgba(r, g, b, a) => ColorInterp::Rgba(
                r.select_anim(idx),
                g.select_anim(idx),
                b.select_anim(idx),
                a.select_anim(idx),
            ),
            ColorInterp::Noop => self,
        })
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

        ColorInterp::Rgba(r.tween(r2), g.tween(g2), b.tween(b2), a.tween(a2))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use druid_widget_nursery::animation::AnimationEvent::Ended;
    use druid_widget_nursery::animation::{Animator, AnimationEvent, AnimationId};
    use crate::interp::InterpHolder::*;
    use crate::vis::{MarkInterp, MarkShapeInterp, TextMarkInterp};
    use crate::{Mark, VisMarks};
    use std::mem::size_of;
    use std::time::Duration;
    use std::num::NonZeroU32;

    #[test]
    fn test_merge() {
        let mut subject = Rect::from_origin_size(Point::ZERO, (100., 100.));
        let mut rect_1 = RectInterp::Coords {
            x0: 0.0.tween(10.0),
            y0: Default::default(),
            x1: Default::default(),
            y1: Default::default(),
        }
        .animation();
        rect_1
            .interp(&AnimationCtx::running(0.5), &mut subject)
            .expect("ok");
        assert_eq!(Rect::new(5., 0., 100., 100.), subject);

        let mut rect_2 = RectInterp::Coords {
            x0: Default::default(),
            y0: 1000.0.tween(2000.0),
            x1: Default::default(),
            y1: Default::default(),
        }
        .animation();
        subject = Rect::from_origin_size(Point::ZERO, (100., 100.));
        rect_2
            .interp(&AnimationCtx::running(0.5), &mut subject)
            .expect("ok");
        assert_eq!(Rect::new(0., 1500., 100., 100.), subject);
        let mut merged = rect_1.merge(rect_2);

        println!("{:#?}", merged);

        subject = Rect::from_origin_size(Point::ZERO, (100., 100.));
        merged
            .interp(&AnimationCtx::running(0.5), &mut subject)
            .expect("ok");

        assert_eq!(Rect::new(5., 1500., 100., 100.), subject);
    }

    #[test]
    fn test_merge_selected_disjoint() {
        let mut animator: Animator = Default::default();
        let mut root_0: InterpNode<Line> = Default::default();

        let ai_0 = animator.new_animation().duration(Duration::from_nanos(100)).id();
        root_0.get().p0.get().x = 0.0.tween(20.0).select_anim(ai_0);
        let mut root_1: InterpNode<Line> = Default::default();

        let ai_1 = animator
            .new_animation()
            .duration(Duration::from_nanos(100))
            .after(Ended(ai_0))
            .id();
        root_1.get().p0.get().y = 100.0.tween(200.0).select_anim(ai_1);

        //let p = std::mem::take(&mut root_0.get().p0).merge(std::mem::take( &mut root_1.get().p0) );
        //panic!("{:#?}", p)

        let merged = root_0.merge(root_1);

        assert!(merged.selected.is_empty());

        match merged.focused {
            Some(Interp(LineInterp {
                p0:
                    InterpNode {
                        focused:
                            Some(Interp(PointInterp {
                                x:
                                    InterpNode {
                                        selected: sel_x, ..
                                    },
                                y:
                                    InterpNode {
                                        selected: sel_y, ..
                                    },
                            })),
                        ..
                    },
                p1: InterpNode { focused: None, .. },
            })) if sel_x[0].0 == ai_0 && sel_y[0].0 == ai_1 => {}
            ex => panic!("{:#?}", ex),
        }
    }

    #[test]
    fn test_merge_selected_overlap() {
        simple_logger::init();
        let mut animator: Animator = Default::default();

        let ai_0 = animator.new_animation().duration(Duration::from_nanos(100)).id();
        let mut root_0: InterpNode<Line> = Default::default();
        root_0.get().p0.get().x = 0.0.tween(20.0);
        root_0 = root_0.select_anim(ai_0);
        // The merge should not care where the select is if its logically the same effect

        let mut root_1: InterpNode<Line> = Default::default();

        let ai_1 = animator
            .new_animation()
            .duration(Duration::from_nanos(100))
            .after(Ended(ai_0))
            .id();
        root_1.get().p0.get().x = 100.0.tween(200.0).select_anim(ai_1);

        let merged = root_0.merge(root_1);
        let str = format!("{:#?}", merged);

        if !(match merged {
            InterpNode {
                focused:
                    Some(Interp(LineInterp {
                        p0:
                            InterpNode {
                                focused:
                                    Some(Interp(PointInterp {
                                        x:
                                            InterpNode {
                                                selected: sa,
                                                focused: Noop,
                                            },
                                        y: InterpNode { focused: None, .. },
                                    })),
                                ..
                            },
                        p1: InterpNode { focused: None, .. },
                    })),
                ..
            } => match sa[..] {
                [(ai_0_ex, _), (ai_1_ex, _)] if ai_0_ex == ai_0 && ai_1_ex == ai_1 => true,
                _ => false,
            },
            ex => false,
        }) {
            panic!("{}", str)
        }
    }

    //#[test]
    fn test_merge_descend() {
        let mut animator: Animator = Default::default();

        let mut line_tw = |v: f64| {
            Line::new((v, v), (v + 5.0, v + 5.0))
                .tween(Line::new((v + 2.0, v + 2.0), (v + 10.0, v + 10.0)))
                .select_anim(animator.new_animation().id())
        };

        // These ones have the exact same structure, so should be in a select many
        let mut root_0: InterpNode<Line> = line_tw(1.);
        let mut root_1: InterpNode<Line> = line_tw(2.);

        let merged = root_0.merge(root_1);

        match merged {
            InterpNode {
                focused:
                    Some(Interp(LineInterp {
                        p0:
                            InterpNode {
                                focused:
                                    Some(Interp(PointInterp {
                                        x:
                                            InterpNode {
                                                selected: sa,
                                                focused: Noop,
                                            },
                                        y: InterpNode { focused: None, .. },
                                    })),
                                ..
                            },
                        p1: InterpNode { focused: None, .. },
                    })),
                ..
            } => (),
            ex => panic!("{:#?}", ex),
        }

        let mut root_0: InterpNode<Line> = line_tw(1.);
        root_0.get().p0.get().x = Default::default();
        let mut root_1: InterpNode<Line> = line_tw(2.);
        let merged = root_0.merge(root_1);

        match merged {
            InterpNode {
                focused:
                    Some(Interp(LineInterp {
                        p0:
                            InterpNode {
                                focused:
                                    Some(Interp(PointInterp {
                                        x:
                                            InterpNode {
                                                selected: sa,
                                                focused: None,
                                            },
                                        y: InterpNode { focused: None, .. },
                                    })),
                                ..
                            },
                        p1: InterpNode { focused: None, .. },
                    })),
                ..
            } => (),
            ex => panic!("{:#?}", ex),
        }
    }

    #[test]
    fn test_select_internal() {
        let mut p1: InterpNode<Point> = Default::default();
        p1.get().x = 1.0.tween(6.7);
        let id = AnimationId::new(900, NonZeroU32::new(6534).unwrap());
        let p_sel = p1.select_anim(id);
        let (res, rem) = select_internal::<Point>(p_sel.selected);

        let matched = match res {
            Some(Interp(PointInterp {
                x:
                    InterpNode {
                        selected: ids,
                        focused: None,
                    },
                y: InterpNode { focused: None, .. },
            })) => match &ids[..] {
                [(
                    found_id,
                    Interp(F64Interp {
                        start: 1.0,
                        end: 6.7,
                    }),
                )] => true,
                _ => false,
            },
            _ => false,
        };

        assert!(matched)
    }

    // Curves
    // Events
    // Loops
    // Segment removal
}
