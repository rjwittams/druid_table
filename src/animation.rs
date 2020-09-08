use druid::im::Vector;
use druid::kurbo::{Line, Point, Rect, Size};
use druid::piet::Color;
use itertools::Itertools;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::num::NonZeroU32;
use std::ops::Add;
use std::time::Duration;

type Nanos = f64;
type DelayNanos = Nanos; // delay after ready
type StartNanos = Nanos; // start time
type Animations = AnimationStorage<AnimationState>;

#[derive(Debug)]
pub struct AnimationCtxInner<'a> {
    focus: Option<AnimationId>,
    animations: &'a Animations,
}

impl AnimationCtxInner<'_> {
    fn with_focused<V>(&self, f: impl Fn(&AnimationState) -> V) -> Option<V> {
        self.focus
            .and_then(|focus| self.animations.get(focus))
            .map(f)
    }
}

#[derive(Debug)]
pub enum AnimationCtx<'a> {
    Full(AnimationCtxInner<'a>),
    Immediate(f64, AnimationStatus),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AnimationStatus {
    NotRunning,
    Enlisting,
    Running,
    Repeating,
    Retiring,
}

impl AnimationCtx<'_> {
    pub fn running(frac: f64) -> AnimationCtx<'static> {
        AnimationCtx::Immediate(frac, AnimationStatus::Running)
    }

    fn new(focus: Option<AnimationId>, animations: &Animations) -> AnimationCtx {
        match focus {
            Some(current_segment) if !animations.contains(current_segment) => panic!(
                "animation segment out of range {:?} {:?}",
                current_segment, animations
            ),
            _ => AnimationCtx::Full(AnimationCtxInner { focus, animations }),
        }
    }

    pub fn progress(&self) -> f64 {
        match self {
            AnimationCtx::Full(inner) => inner.with_focused(|seg| seg.progress).unwrap_or(0.),
            AnimationCtx::Immediate(progress, ..) => *progress,
        }
    }

    pub fn clamped(&self) -> f64 {
        clamp_fraction(self.progress())
    }

    pub fn status(&self) -> AnimationStatus {
        match self {
            AnimationCtx::Full(inner) => inner
                .with_focused(|seg| seg.status())
                .unwrap_or(AnimationStatus::NotRunning),
            AnimationCtx::Immediate(_, status) => *status,
        }
    }

    pub fn with_animation<V>(
        &self,
        id: impl Into<Option<AnimationId>>,
        mut f: impl FnMut(&AnimationCtx) -> V,
    ) -> Option<V> {
        let id_opt = id.into();
        match self {
            AnimationCtx::Full(AnimationCtxInner { animations, .. })
                if id_opt
                    .and_then(|ai| animations.get(ai).map(|s| s.status.is_active()))
                    .unwrap_or(false) =>
            {
                Some(f(&Self::new(id_opt, animations)))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AnimationStatusInternal {
    PendingEvent(DelayNanos), // delay after ready
    Waiting(StartNanos),
    Enlisting(StartNanos),
    Running(StartNanos),
    Repeating(StartNanos), // Start of current repetition
    Retiring,
}

impl AnimationStatusInternal {
    fn is_active(&self) -> bool {
        match self {
            AnimationStatusInternal::Enlisting(_)
            | AnimationStatusInternal::Running(_)
            | AnimationStatusInternal::Retiring => true,
            _ => false,
        }
    }

    fn add_delay(&self, cur_nanos: f64, delay_nanos: f64, duration: f64) -> Self {
        match self {
            AnimationStatusInternal::PendingEvent(delay) => {
                AnimationStatusInternal::PendingEvent(delay + delay_nanos)
            }
            AnimationStatusInternal::Waiting(start) => {
                let start = start + delay_nanos;

                if cur_nanos > start + duration {
                    // Skip entirely?
                    AnimationStatusInternal::Retiring
                } else {
                    AnimationStatusInternal::Waiting(start)
                }
            }
            AnimationStatusInternal::Enlisting(start)
            | AnimationStatusInternal::Repeating(start)
            | AnimationStatusInternal::Running(start) => {
                let start = start + delay_nanos;

                if start > cur_nanos {
                    AnimationStatusInternal::Running(start)
                } else {
                    // Could enlist twice - would need to have a WaitingEnlisted state to prevent
                    AnimationStatusInternal::Waiting(start)
                }
            }
            AnimationStatusInternal::Retiring => AnimationStatusInternal::Retiring,
            // Does this need to have a pre-retiring state to make sure interps run once
            // (to do their retirement actions)
        }
    }

    fn pending(&self, cur_nanos: f64) -> Self {
        match self {
            AnimationStatusInternal::Waiting(start)
            | AnimationStatusInternal::Enlisting(start)
            | AnimationStatusInternal::Running(start) => {
                AnimationStatusInternal::PendingEvent((cur_nanos - start).min(0.))
            }
            other => other.clone(),
        }
    }
}

pub enum CustomCurve {
    Function(fn(f64) -> f64),
    Boxed(Box<dyn FnMut(f64) -> f64>),
}

impl CustomCurve {
    fn translate(&mut self, t: f64) -> f64 {
        match self {
            CustomCurve::Function(f) => f(t),
            CustomCurve::Boxed(f) => f(t),
        }
    }
}

fn clamp_fraction(f: f64) -> f64 {
    // clamp is unstable
    f.max(0.).min(1.)
}

impl Debug for CustomCurve {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CustomCurve::Function(f) => formatter
                .debug_struct("CustomAnimationCurve::Function")
                .field("f", f)
                .finish(),
            CustomCurve::Boxed(_) => formatter
                .debug_struct("CustomAnimationCurve::Closure")
                .finish(),
        }
    }
}

impl From<fn(f64) -> f64> for AnimationCurve {
    fn from(f: fn(f64) -> f64) -> Self {
        AnimationCurve::Custom(CustomCurve::Function(f))
    }
}

impl From<SimpleCurve> for AnimationCurve {
    fn from(s: SimpleCurve) -> Self {
        AnimationCurve::Simple(s)
    }
}

#[derive(Data, Copy, Clone, Debug, Eq, PartialEq)]
pub enum SimpleCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    OutElastic,
    OutBounce,
    OutSine,
}

impl SimpleCurve {
    fn translate(&mut self, t: f64) -> f64 {
        use std::f64::consts::PI;
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                let t = t * 2.0;
                if t < 1. {
                    0.5 * t * t
                } else {
                    let t = t - 1.;
                    -0.5 * (t * (t - 2.) - 1.)
                }
            }
            Self::OutElastic => {
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
            Self::OutSine => (t * PI * 0.5).sin(),
            Self::OutBounce => {
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
        }
    }
}

#[derive(Debug)]
pub enum AnimationCurve {
    Simple(SimpleCurve),
    //    CubicBezier(CubicBezierAnimationCurve),
    //    Spring(SpringAnimationCurve),
    Custom(CustomCurve),
}

impl Default for AnimationCurve {
    fn default() -> Self {
        AnimationCurve::Simple(SimpleCurve::Linear)
    }
}

impl AnimationCurve {
    fn translate(&mut self, t: f64) -> f64 {
        match self {
            Self::Simple(s) => s.translate(t),
            Self::Custom(c) => c.translate(t),
        }
    }
}

#[derive(Debug, Data, Copy, Clone, PartialOrd, PartialEq)]
pub enum AnimationDirection {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

impl Default for AnimationDirection {
    fn default() -> Self {
        Self::Normal
    }
}

impl AnimationDirection {
    fn translate(&self, frac: f64, even_repeat: bool) -> f64 {
        match self {
            Self::Normal => frac,
            Self::Reverse => 1.0 - frac,
            Self::Alternate => {
                if even_repeat {
                    frac
                } else {
                    1.0 - frac
                }
            }
            Self::AlternateReverse => {
                if !even_repeat {
                    frac
                } else {
                    1.0 - frac
                }
            }
        }
    }

    fn end_fraction(&self, even_repeat: bool) -> f64 {
        match self {
            Self::Normal => 1.0,
            Self::Reverse => 0.0,
            Self::Alternate => {
                if even_repeat {
                    1.
                } else {
                    0.
                }
            }
            Self::AlternateReverse => {
                if !even_repeat {
                    1.
                } else {
                    0.
                }
            }
        }
    }
}

#[derive(Debug)]
struct AnimationState {
    dur_nanos: f64,
    curve: AnimationCurve,
    direction: AnimationDirection,
    repeat_limit: Option<usize>,
    status: AnimationStatusInternal,
    since_start: f64,
    fraction: f64,
    progress: f64,
    repeat_count: usize,
}

impl AnimationState {
    pub fn new(status: AnimationStatusInternal) -> Self {
        AnimationState {
            dur_nanos: 1.,
            curve: Default::default(),
            direction: Default::default(),
            repeat_limit: Some(1),
            status,
            since_start: 0.,
            fraction: 0.,
            progress: 0.,
            repeat_count: 0,
        }
    }

    fn calc(&mut self, cur_nanos: Nanos) {
        let before_end = self.since_start < self.dur_nanos; // Ask curve (e.g non duration based)

        let even_repeat = self.repeat_count % 2 == 0;

        if before_end {
            self.fraction = self
                .direction
                .translate(self.since_start / self.dur_nanos, even_repeat);
            self.progress = self.curve.translate(self.fraction);
        } else {
            // This animation will go through one more cycle to give interps
            // a chance to recover from any discontinuous curves - i.e set things to the end state.

            self.repeat_count += 1;
            let allow_repeat = self
                .repeat_limit
                .map_or(true, |limit| self.repeat_count < limit);
            if allow_repeat {
                self.status = AnimationStatusInternal::Repeating(cur_nanos);
            } else {
                let end_fraction = self.direction.end_fraction(even_repeat);
                self.fraction = end_fraction;
                self.progress = end_fraction;
                self.status = AnimationStatusInternal::Retiring;
            }
        }
    }

    fn advance(&mut self, cur_nanos: f64) -> bool {
        use AnimationStatusInternal::*;
        match self.status.clone() {
            Waiting(start) => {
                self.since_start = cur_nanos - start;
                if self.since_start > 0. {
                    self.status = AnimationStatusInternal::Enlisting(start);
                    // TODO priming state for first run
                    self.calc(cur_nanos);
                }
                false
            }
            Enlisting(start) | Repeating(start) => {
                self.since_start = cur_nanos - start;
                self.status = AnimationStatusInternal::Running(start);
                self.calc(cur_nanos);
                false
            }
            Running(start) => {
                self.since_start = cur_nanos - start;
                self.calc(cur_nanos);
                false
            }
            Retiring => true,
            PendingEvent(_) => false,
        }
    }

    // Might be able to merge these
    fn status(&self) -> AnimationStatus {
        match self.status {
            AnimationStatusInternal::PendingEvent(_) => AnimationStatus::NotRunning,
            AnimationStatusInternal::Waiting(_) => AnimationStatus::NotRunning,
            AnimationStatusInternal::Enlisting(_) => AnimationStatus::Enlisting,
            AnimationStatusInternal::Repeating(_) => AnimationStatus::Repeating,
            AnimationStatusInternal::Running(_) => AnimationStatus::Running,
            AnimationStatusInternal::Retiring => AnimationStatus::Retiring,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub enum AnimationEvent {
    Named(&'static str),
    Ended(AnimationId),
}

type ASOffset = u32;
type ASVersion = NonZeroU32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct AnimationId {
    offset: ASOffset,
    version: ASVersion,
}

impl AnimationId {
    pub fn new(offset: ASOffset, version: NonZeroU32) -> Self {
        AnimationId { offset, version }
    }
}

#[derive(Debug)]
enum ASEntry<Value> {
    Busy(ASVersion, Value),
    Free(ASVersion, ASOffset), // next free. Free entries form a linked list.
    LastFree(ASVersion),
}

#[derive(Debug)]
struct AnimationStorage<Value> {
    contents: Vec<ASEntry<Value>>,
    size: ASOffset,
    first_free: Option<ASOffset>,
}

//Derive creates an incorrect constraint
impl<Value> Default for AnimationStorage<Value> {
    fn default() -> Self {
        AnimationStorage {
            contents: Default::default(),
            size: Default::default(),
            first_free: Default::default(),
        }
    }
}

impl<Value> AnimationStorage<Value> {
    fn iter(&self) -> impl Iterator<Item = &Value> {
        self.contents.iter().flat_map(|content| match content {
            ASEntry::Busy(_, seg) => Some(seg),
            _ => None,
        })
    }

    // O(n)
    fn remove_if(&mut self, mut f: impl FnMut(AnimationId, &mut Value) -> bool) {
        for (offset, entry) in self.contents.iter_mut().enumerate() {
            let offset = offset as ASOffset;
            let (version, remove) = match entry {
                ASEntry::Busy(version, value) => (
                    version.clone(),
                    f(AnimationId::new(offset, *version), value),
                ),
                ASEntry::Free(version, _) | ASEntry::LastFree(version) => (*version, false),
            };

            if remove {
                *entry = self
                    .first_free
                    .map(|next_free| ASEntry::Free(version, next_free))
                    .unwrap_or_else(|| ASEntry::LastFree(version));
                self.first_free = Some(offset);
                self.size -= 1;
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.size == 0
    }

    // O(1)
    fn insert(&mut self, value: Value) -> AnimationId {
        self.size += 1;
        if let Some(offset) = self.first_free.take() {
            let entry = &mut self.contents[offset as usize];
            let (first_free, version) = match entry {
                ASEntry::LastFree(version) => (None, version),
                ASEntry::Free(version, next_free) => (Some(*next_free), version),
                ASEntry::Busy(..) => panic!("Free list pointing to busy entry"),
            };
            self.first_free = first_free;
            let version = NonZeroU32::new(version.get().wrapping_add(1).max(1)).unwrap();
            *entry = ASEntry::Busy(version, value);
            AnimationId::new(offset, version)
        } else {
            let version = NonZeroU32::new(1).unwrap();
            let id = AnimationId::new(self.contents.len() as u32, version);
            self.contents.push(ASEntry::Busy(version, value));
            id
        }
    }

    // O(1)
    fn contains(&self, id: AnimationId) -> bool {
        id.offset < self.contents.len() as u32
            && matches!(self.contents[id.offset as usize], ASEntry::Busy(version, _) if version == id.version)
    }

    // O(1)
    fn get(&self, id: AnimationId) -> Option<&Value> {
        self.contents
            .get(id.offset as usize)
            .and_then(|entry| match entry {
                ASEntry::Busy(version, seg) if *version == id.version => Some(seg),
                _ => None,
            })
    }

    fn get_mut(&mut self, id: AnimationId) -> Option<&mut Value> {
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

pub struct AnimationHandle<'a> {
    id: AnimationId,
    animator: &'a mut Animator,
}

impl AnimationHandle<'_> {
    fn change_animation_state(self, f: impl FnOnce(&mut AnimationState)) -> Self {
        self.animator
            .storage
            .get_mut(self.id)
            .map(f)
            .unwrap_or_else(|| log::warn!("Attempt to modify retired segment {:?}", self.id));
        self
    }

    pub fn delay(self, delay: impl Into<Duration>) -> Self {
        let cur_nanos = self.animator.cur_nanos;
        let delay = delay.into().as_nanos() as f64;
        self.change_animation_state(|seg| {
            seg.status = seg.status.add_delay(cur_nanos, delay, seg.dur_nanos);
        })
    }

    pub fn duration(self, duration: impl Into<Duration>) -> Self {
        self.change_animation_state(|seg| seg.dur_nanos = duration.into().as_nanos() as f64)
    }

    pub fn direction(self, direction: impl Into<AnimationDirection>) -> Self {
        let dir = direction.into();
        self.change_animation_state(|state| state.direction = dir)
    }

    pub fn repeat_limit(self, limit: impl Into<Option<usize>>) -> Self {
        let limit = limit.into();
        self.change_animation_state(|state| state.repeat_limit = limit)
    }

    pub fn curve(self, curve: impl Into<AnimationCurve>) -> Self {
        let curve = curve.into();
        self.change_animation_state(|seg| seg.curve = curve)
    }

    pub fn after(self, event: impl Into<AnimationEvent>) -> Self {
        self.animator.register_pending(event.into(), self.id);
        let cur_nanos = self.animator.cur_nanos;

        self.change_animation_state(|seg| seg.status = seg.status.pending(cur_nanos))
    }

    pub fn id(&self) -> AnimationId {
        self.id
    }

    pub fn is_valid(&self) -> bool {
        self.animator.storage.contains(self.id)
    }

    pub fn status(&self) -> AnimationStatus {
        self.animator
            .storage
            .get(self.id)
            .map_or(AnimationStatus::NotRunning, |state| state.status())
    }
}

#[derive(Default, Debug)]
pub struct Animator {
    cur_nanos: Nanos,
    pending_count: u32,
    pending_starts: HashMap<AnimationEvent, Vec<AnimationId>>,
    storage: AnimationStorage<AnimationState>,
}

impl Animator {
    pub fn advance_by<V>(
        &mut self,
        nanos: Nanos,
        mut interpolate: impl FnMut(&AnimationCtx) -> V,
    ) -> Option<V> {
        if self.storage.is_empty() {
            None
        } else {
            self.cur_nanos += nanos;

            let mut pending_events = VecDeque::new();

            let res = {
                let cur_nanos = self.cur_nanos;

                self.storage.remove_if(|id, segment| {
                    let remove = segment.advance(cur_nanos);
                    if remove {
                        pending_events.push_back(AnimationEvent::Ended(id))
                    }
                    remove
                });

                let ctx = AnimationCtx::new(None, &self.storage);
                interpolate(&ctx)
            };

            for event in pending_events.into_iter() {
                self.event(event)
            }

            if self.storage.is_empty() {
                self.cur_nanos = 0.;
            }
            Some(res)
        }
    }

    pub fn event(&mut self, event: AnimationEvent) {
        if let Some(ids) = self.pending_starts.remove(&event) {
            for id in ids {
                if let Some(seg) = self.storage.get_mut(id) {
                    if let AnimationStatusInternal::PendingEvent(delay) = seg.status.clone() {
                        self.pending_count -= 1;
                        seg.status = AnimationStatusInternal::Waiting(self.cur_nanos + delay);
                    }
                }
            }
        };
    }

    fn register_pending(&mut self, event: AnimationEvent, id: AnimationId) {
        self.pending_starts
            .entry(event)
            .or_insert_with(|| vec![])
            .push(id);
        self.pending_count += 1;
    }

    pub fn running(&self) -> bool {
        // TODO: If we had waiting ones we could return a minimum time until one had to start
        // Could maintain a max wait time
        (self.storage.size - self.pending_count) > 0
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn new(&mut self) -> AnimationHandle {
        let id = self
            .storage
            .insert(AnimationState::new(AnimationStatusInternal::Waiting(
                self.cur_nanos,
            )));
        AnimationHandle { id, animator: self }
    }

    pub fn new_configured(&mut self, f: impl Fn(AnimationHandle)) -> AnimationHandle {
        let mut handle = self.new();
        let id = handle.id();
        f(handle);
        self.get(id)
    }

    pub fn get(&mut self, id: AnimationId) -> AnimationHandle {
        AnimationHandle { id, animator: self }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::animation::AnimationEvent::Ended;
    use crate::interp::InterpHolder::*;
    use crate::interp::{HasInterp, InterpNode, InterpResult};
    use crate::vis::{MarkInterp, MarkShapeInterp, TextMarkInterp};
    use crate::{Mark, VisMarks};
    use std::mem::size_of;

    #[test]
    fn test_animator() {
        let mut animator: Animator = Default::default();

        let ai_0 = animator.new().duration(Duration::from_nanos(100)).id();

        let ai_1 = animator
            .new()
            .duration(Duration::from_nanos(100))
            .after(AnimationEvent::Ended(ai_0))
            .id();

        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        let mut advance = |animator: &mut Animator, nanos: f64| -> (Option<f64>, Option<f64>) {
            let res = animator.advance_by(nanos, |ctx| {
                (
                    ctx.with_animation(ai_0, |ctx| ctx.progress()),
                    ctx.with_animation(ai_1, |ctx| ctx.progress()),
                )
            });
            res.unwrap()
        };

        assert_eq!((Some(0.5), None), advance(&mut animator, 50.0));

        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        // Advance just beyond the first animations end.
        // It will be retiring (and forced to 1.0)
        // The second will still be waiting
        assert_eq!((Some(1.0), None), advance(&mut animator, 50.1));

        assert_eq!(
            AnimationStatusInternal::Retiring,
            animator.storage.get(ai_0).unwrap().status
        );
        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        advance(&mut animator, 1.);
        // Second animation is now
        assert_eq!(
            AnimationStatusInternal::Waiting(101.1),
            animator.storage.get(ai_1).unwrap().status
        );

        assert_eq!((None, Some(0.1)), advance(&mut animator, 10.));
    }

    #[test]
    fn test_animator_interp() {
        let mut animator: Animator = Default::default();
        let mut root: InterpNode<Line> = Default::default();
        let mut p_0 = &mut root.get().p0.get();

        let ai_0 = animator.new().duration(Duration::from_nanos(100)).id;
        p_0.x = 0.0.tween(20.0).select_anim(ai_0);

        let ai_1 = animator
            .new()
            .duration(Duration::from_nanos(100))
            .after(AnimationEvent::Ended(ai_0))
            .id;
        p_0.y = 100.0.tween(200.0).select_anim(ai_1);
        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        let mut my_line = Line::new((0.0, 0.0), (100.0, 100.0));
        let mut advance = |animator: &mut Animator, line: &mut Line, nanos: f64| {
            let res = animator.advance_by(nanos, |ctx| root.interp(ctx, line));
            res
        };

        advance(&mut animator, &mut my_line, 50.0);
        assert_eq!(Line::new((10.0, 0.0), (100.0, 100.0)), my_line);
        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        advance(&mut animator, &mut my_line, 50.1);
        assert_eq!(Line::new((20.0, 0.0), (100.0, 100.0)), my_line);
        assert_eq!(
            AnimationStatusInternal::Retiring,
            animator.storage.get(ai_0).unwrap().status
        );
        assert_eq!(
            AnimationStatusInternal::PendingEvent(0.),
            animator.storage.get(ai_1).unwrap().status
        );

        advance(&mut animator, &mut my_line, 1.);
        assert_eq!(
            AnimationStatusInternal::Waiting(101.1),
            animator.storage.get(ai_1).unwrap().status
        );
        advance(&mut animator, &mut my_line, 10.);
        assert_eq!(Line::new((20.0, 110.0), (100.0, 100.0)), my_line);
    }

    // Curves
    // Events
    // Loops
    // Segment removal
}
