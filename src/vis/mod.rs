use crate::animation::*;
use crate::LogIdx;
use druid::kurbo::{Affine, Line, ParamCurveNearest, Point, Rect, Size, Vec2};
use druid::piet::{FontFamily, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::RenderContext;
use druid::{
    BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, UpdateCtx, Widget,
};
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::LN_10;
use std::f64::NAN;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::marker::PhantomData;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct TextMarkInterp {
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

    fn coverage(&self) -> InterpCoverage {
        self.point.coverage() + self.size.coverage() + self.txt.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
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
pub enum MarkShapeInterp {
    Rect(Animation<Rect>),
    Line(Animation<Point>, Animation<Point>),
    Text(Animation<TextMark>),
    Noop,
}

impl Default for MarkShapeInterp {
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
            (MarkShapeInterp::Rect(r_int), MarkShape::Rect(rect)) => r_int.prime(rect),
            (MarkShapeInterp::Line(o, other), MarkShape::Line(l)) => {
                o.prime(&mut l.p0)?;
                other.prime(&mut l.p1)
            }
            (MarkShapeInterp::Text(t_interp), MarkShape::Text(t)) => t_interp.prime(t),
            _ => Err(InterpError::ValueMismatch),
        }
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut MarkShape) -> InterpResult {
        match (self, val) {
            (MarkShapeInterp::Rect(r_int), MarkShape::Rect(r)) => {
                // TODO: Do coords not points
                r_int.interp(frac, r)
            }
            (MarkShapeInterp::Line(o, other), MarkShape::Line(l)) => {
                o.interp(frac, &mut l.p0)?;
                other.interp(frac, &mut l.p1)
            }
            (MarkShapeInterp::Text(t_interp), MarkShape::Text(t)) => t_interp.interp(frac, t),
            _ => Err(InterpError::ValueMismatch),
        }
    }

    fn coverage(&self) -> InterpCoverage {
        match self {
            MarkShapeInterp::Rect(rect) => rect.coverage(),
            MarkShapeInterp::Line(start, end) => start.coverage() + end.coverage(),
            MarkShapeInterp::Text(text) => text.coverage(),
            MarkShapeInterp::Noop => InterpCoverage::Noop,
        }
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        match self {
            MarkShapeInterp::Rect(rect) => MarkShapeInterp::Rect(rect.select_anim(idx)),
            MarkShapeInterp::Line(start, end) => {
                MarkShapeInterp::Line(start.select_anim(idx), end.select_anim(idx))
            }
            MarkShapeInterp::Text(text) => MarkShapeInterp::Text(text.select_anim(idx)),
            other => other,
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (MarkShapeInterp::Rect(rect1), MarkShapeInterp::Rect(rect2)) => {
                MarkShapeInterp::Rect(rect1.merge(rect2))
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
        match (old, new) {
            (o, n) if o.same(&n) => MarkShapeInterp::Noop,
            (MarkShape::Rect(o), MarkShape::Rect(n)) => MarkShapeInterp::Rect(o.tween(n)),
            (MarkShape::Line(o), MarkShape::Line(n)) => {
                MarkShapeInterp::Line(o.p0.tween(n.p0), o.p1.tween(n.p1))
            }
            (MarkShape::Text(o), MarkShape::Text(n)) => MarkShapeInterp::Text(o.tween(n)),
            _ => MarkShapeInterp::Noop,
        }
    }
}

#[derive(Debug, Default)]
pub struct MarkInterp {
    shape: Animation<MarkShape>,
    current: Animation<MarkProps>,
}

#[derive(Debug, Data, Clone)]
pub struct MarkOverrides {
    color: Option<Color>,
}

impl MarkOverrides {
    pub fn new(color: impl Into<Option<Color>>) -> Self {
        MarkOverrides {
            color: color.into(),
        }
    }
}

impl MarkOverrides {
    fn apply(&self, props: &mut MarkProps) {
        if let Some(col) = &self.color {
            props.color = col.clone();
        }
    }
}

#[derive(Debug, Data, Clone)]
pub struct MarkProps {
    color: Color,
}

impl MarkProps {
    pub fn new(color: Color) -> Self {
        MarkProps { color }
    }
}

impl Default for MarkProps {
    fn default() -> Self {
        MarkProps {
            color: Color::BLACK.with_alpha(0.),
        }
    }
}

impl HasInterp for MarkProps {
    type Interp = MarkPropsInterp;
}

#[derive(Debug, Default)]
pub struct MarkPropsInterp {
    color: Animation<Color>,
}

impl Interp for MarkPropsInterp {
    type Value = MarkProps;

    fn prime(&mut self, val: &mut Self::Value) -> InterpResult {
        OK
    }

    fn interp(&self, frac: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.color.interp(frac, &mut val.color)
    }

    fn coverage(&self) -> InterpCoverage {
        self.color.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        Self {
            color: self.color.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            color: self.color.merge(other.color),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        MarkPropsInterp {
            color: start.color.tween(end.color),
        }
    }
}

#[derive(Debug, Data, Clone)]
pub struct Mark {
    id: MarkId,
    shape: MarkShape,
    original: MarkProps,
    hover: Option<MarkOverrides>,
    current: MarkProps,
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
        self.current.interp(frac, &mut val.current)?;
        OK
    }

    fn coverage(&self) -> InterpCoverage {
        self.shape.coverage() + self.current.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
        let MarkInterp { shape, current } = self;
        MarkInterp {
            shape: shape.select_anim(idx),
            current: current.select_anim(idx),
        }
    }

    fn merge(self, other: Self) -> Self {
        let MarkInterp { shape, current } = self;
        let MarkInterp {
            shape: s1,
            current: c1,
        } = other;
        MarkInterp {
            shape: shape.merge(s1),
            current: current.merge(c1),
        }
    }

    fn build(old: Mark, new: Mark) -> Self {
        MarkInterp {
            shape: old.shape.tween(new.shape),
            current: old.current.tween(new.current),
        }
    }
}

impl EnterExit for Mark {
    fn enter(&self) -> Self {
        let shape = match &self.shape {
            MarkShape::Rect(r) => MarkShape::Rect(Rect::from_center_size(r.center(), Size::ZERO)),
            MarkShape::Line(l) => {
                let mid = l.p0.tween_now(l.p1, 0.5).unwrap();
                MarkShape::Line(Line::new(mid, mid))
            }
            s => s.clone(),
        };
        let mut enter_props = self.original.clone();
        enter_props.color = enter_props.color.with_alpha(0.);

        Mark::new_with_current(
            self.id,
            shape,
            enter_props,
            self.original.clone(),
            self.hover.clone(),
        )
    }

    fn exit(&self) -> Self {
        self.enter()
    }
}

impl Mark {
    pub fn hover_props(&self) -> MarkProps {
        let mut props = self.current.clone();
        if let Some(hv) = &self.hover {
            hv.apply(&mut props);
        }
        props
    }

    pub fn new(
        id: MarkId,
        shape: MarkShape,
        original: MarkProps,
        hover: impl Into<Option<MarkOverrides>>,
    ) -> Self {
        let current = original.clone();
        Mark {
            id,
            shape,
            original,
            current,
            hover: hover.into(),
        }
    }

    pub fn new_with_current(
        id: MarkId,
        shape: MarkShape,
        current: MarkProps,
        original: MarkProps,
        hover: impl Into<Option<MarkOverrides>>,
    ) -> Self {
        Mark {
            id,
            shape,
            current,
            original,
            hover: hover.into(),
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

    pub fn paint(&self, ctx: &mut PaintCtx) {
        let color = &self.current.color;
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
pub struct VisMarks {
    // TODO: slotmap?
    marks: HashMap<MarkId, Mark>,
}

impl VisMarks {
    fn find_mark(&self, pos: Point) -> Option<&Mark> {
        self.marks.values().filter(|mark| mark.hit(pos)).next()
    }

    fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        for (_, mark) in self.marks.iter().sorted_by_key(|(k, v)| k.clone()) {
            mark.paint(ctx);
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
pub struct VisMarksInterp {
    pub marks: Animation<HashMap<MarkId, Mark>>,
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

    fn coverage(&self) -> InterpCoverage {
        self.marks.coverage()
    }

    fn select_animation_segment(self, idx: AnimationSegmentId) -> Self {
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
            marks: MapInterp::build(old.marks, new.marks).animation(),
        }
    }
}

struct VisInner<VP: Visualization> {
    layout: VP::Layout,
    state: VP::State,
    animator: Animator<VisMarks>,
    current: VisMarks,
    transform: Affine,
    hovered: Option<MarkId>,
    phantom_vp: PhantomData<VP>,
}

impl<VP: Visualization> VisInner<VP> {
    pub fn new(
        layout: VP::Layout,
        state: VP::State,
        animator: Animator<VisMarks>,
        current: VisMarks,
        transform: Affine,
    ) -> Self {
        VisInner {
            layout,
            state,
            animator,
            current,
            transform,
            hovered: None,
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
            let state_marks = Default::default(); //self.visual.state_marks(data, &layout, &state);
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

    fn regenerate(
        &mut self,
        size: Size,
        data: &V::Input,
    ) -> Result<AnimationSegmentId, InterpError> {
        if let Some(inner) = &mut self.inner {
            inner.layout = self.visual.layout(data, size);

            let destination = VisMarks::build(
                self.visual.layout_marks(&inner.layout),
                Default::default(), //self.visual.state_marks(data, &inner.layout, &inner.state),
                self.visual.data_marks(data, &inner.layout),
            );

            let interp = inner.current.clone().tween(destination);
            let id = inner.animator
                .segment()
                .duration(Duration::from_millis(1000) )
                .curve(AnimationCurve::EaseIn)
                .id();

            let selected = interp.select_anim(id);

            inner.animator.merge_animation(selected, &mut inner.current).map(|_|id)
        } else {
            Err(InterpError::NotRunning)
        }
    }
}

impl<V: Visualization> Widget<V::Input> for Vis<V> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut V::Input, env: &Env) {
        self.ensure_inner(data, ctx.size());
        let inner = self.inner.as_mut().unwrap();

        let visual = &mut self.visual;
        let VisInner {
            animator,
            current,
            hovered,
            layout,
            state,
            transform,
            ..
        } = inner;
        let old_state: V::State = state.clone();

        fn unhover<V: Visualization>(
            animator: &mut Animator<VisMarks>,
            hovered: &mut Option<MarkId>,
            visual: &mut V,
            data: &mut V::Input,
            layout: &V::Layout,
            state: &mut V::State,
        ) {
            if let Some(focus) = hovered {
                visual.event(data, layout, state, &VisEvent::MouseOut(*focus));
                animator.event(Vis::<V>::UNHOVER);
            }
            *hovered = None;
        }

        match event {
            Event::MouseMove(me) => {
                if let Some(mark) = current.find_mark(transform.inverse() * me.pos) {
                    if *hovered != Some(mark.id) {
                        unhover(animator, hovered, visual, data, layout, state);
                        visual.event(
                            data,
                            &inner.layout,
                            &mut inner.state,
                            &VisEvent::MouseEnter(mark.id),
                        );
                        *hovered = Some(mark.id);

                        if mark.hover.is_some() {
                            let hover_idx = animator
                                .segment()
                                .duration(Duration::from_millis(1250))
                                .id();
                            let hover_props = mark.hover_props();
                            let color_change =
                                mark.current.tween_ref(&hover_props).select_anim(hover_idx);

                            let unhover_idx = animator
                                .segment()
                                .duration(Duration::from_millis(2500))
                                .curve(AnimationCurve::EaseOut)
                                .after(Self::UNHOVER)
                                .id();

                            let change_back = hover_props
                                .tween_ref(&mark.original)
                                .select_anim(unhover_idx);

                            let mut top_level = Animation::<VisMarks>::default();
                            top_level.get().marks.get().get(&mark.id).get().current =
                                color_change.merge(change_back);



                            animator.merge_animation(top_level, current);
                            //log::info!( "Animator after hover {:#?}", animator);
                        }
                    }
                } else {
                    unhover(animator, hovered, visual, data, layout, state);
                }
            }
            _ => {}
        }

        if !old_state.same(&inner.state) {
            visual
                .state_marks(data, &inner.layout, &inner.state)
                .into_iter()
                .for_each(|mark| {
                    let anim_idx = animator
                        .segment()
                        .duration(Duration::from_secs(3))
                        .curve(AnimationCurve::OutElastic)
                        .id();
                    let id = mark.id;
                    let start = current.marks.entry(id).or_insert(mark.enter());
                    *animator.animation.get().marks.get().get(&id) =
                         start.clone().tween(mark).select_anim(anim_idx);
                })
        }

        if animator.running() {
            ctx.request_anim_frame()
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &V::Input, env: &Env) {
        if let (
            LifeCycle::AnimFrame(nanos),
            Some(VisInner {
                animator, current, ..
            }),
        ) = (event, &mut self.inner)
        {
            let res = animator.advance((*nanos) as f64, current);
            if let Result::Err(e) = res {
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
            self.regenerate(ctx.size(), data);
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
            state.current.paint(ctx, &state.hovered);
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
            MarkProps::new(Color::WHITE),
            None,
        ));
        for (v, p_id) in self.bands.iter() {
            let tick_loc = TickLocator::Persistent(*p_id);
            let b_mid = self.range_val(v).mid();
            marks.push(Mark::new(
                MarkId::Tick(self.name, tick_loc),
                MarkShape::Line(Line::new((b_mid, tick_extent), (b_mid, line_y))),
                MarkProps::new(Color::WHITE),
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
                MarkProps::new(Color::WHITE),
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
            MarkProps::new(Color::WHITE),
            None,
        ));

        for step in 0..=self.ticks {
            let d_v = self.domain_range.0 + self.tick_step * (step as f64);
            let value = T::from_f64(d_v);

            let r_v = self.range_val_raw(d_v);
            marks.push(Mark::new(
                MarkId::Tick(self.name, TickLocator::U64Bits(d_v.to_bits())),
                MarkShape::Line(Line::new((tick_extent, r_v), (line_x, r_v))),
                MarkProps::new(Color::WHITE),
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
                MarkProps::new(Color::WHITE),
                None,
            ));
        }
        DrawableAxis::new(marks)
    }
}
