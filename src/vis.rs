use crate::LogIdx;
use druid::kurbo::{
    Affine, Line, ParamCurveCurvature, ParamCurveDeriv, ParamCurveNearest, Point, Rect, Size, Vec2,
};
use druid::piet::{FontFamily, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::RenderContext;
use druid::widget::{Axis, CrossAxisAlignment};
use druid::{
    BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, UpdateCtx, Value, Widget,
};
use float_ord::FloatOrd;
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::LN_10;
use std::f64::NAN;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::marker::PhantomData;
use std::os::macos::raw::stat;
use std::thread::current;

// Could new type with bounds check
// Number between 0 and 1
type Frac = f64;

#[derive(Debug, Data, Clone)]
pub struct Mark {
    id: MarkId,
    shape: MarkShape,
    color: Color,
    hover: Option<Color>, // Maybe a bunch more properties / states, but could be dependent on state or something?
                          // Could be somewhere else perhaps
}

trait Interp: Sized{
    type Out;
    fn interp(&self, frac: Frac) -> Self::Out;
    fn wrap(self)->ConstOr<Self>{
        ConstOr::Interp(self)
    }
    fn const_val(self)->Option<Self::Out>{
        None
    }
    fn is_const(&self)->bool{
        false
    }
}

enum ConstOr<P: Interp> {
    Const(P::Out),
    Interp(P)
}

impl <T: Clone, P: Interp<Out=T>> From<T> for ConstOr<P> {
    fn from(t: P::Out) -> Self {
        ConstOr::Const(t)
    }

}

impl<P: Interp> Interp for ConstOr<P> where P::Out : Clone {
    type Out = P::Out;
    fn interp(&self, frac: f64) -> P::Out {
        match self {
            ConstOr::Const(t) => t.clone(),
            ConstOr::Interp(i) => i.interp(frac)
        }
    }

    fn const_val(self)->Option<P::Out>{
        if let ConstOr::Const(c) = self{
            Some(c)
        }else{
            None
        }
    }

    fn is_const(&self)->bool{
        matches!(self, ConstOr::Const(_))
    }
}

struct VecInterp<TInterp> {
    interps: Vec<TInterp>
}

impl <TInterp: Interp> VecInterp<TInterp> {
    pub fn new(interps: Vec<TInterp>) -> ConstOr<Self> {
        if interps.iter().all(|i|i.is_const()){
            ConstOr::Const( interps.into_iter().flat_map(|i|i.const_val() ).collect() )
        }else {
            VecInterp {
                interps
            }.wrap()
        }
    }
}

impl <TInterp: Interp> Interp for VecInterp<TInterp> {
    type Out = Vec<TInterp::Out>;
    fn interp(&self, frac: f64) -> Self::Out {
        self.interps.iter().map(|i| i.interp(frac)).collect()
    }
}

enum F64Interp {
    Linear(f64, f64),
}

impl F64Interp{
    fn linear(start: f64, end: f64)->ConstOr<F64Interp>{
        if start == end{
            start.into()
        }else{
            F64Interp::Linear(start, end).wrap()
        }
    }
}

impl Interp for F64Interp {
    type Out = f64;
    fn interp(&self, frac: f64) -> f64 {
        match self {
            F64Interp::Linear(start, end) => start + (end - start) * frac,
        }
    }

    fn wrap(self) -> ConstOr<Self> {
        match self{
            F64Interp::Linear(s, e) if s == e => s.into(),
            _=>ConstOr::Interp(self)
        }
    }
}

enum PointInterp {
    Point(ConstOr<F64Interp>, ConstOr<F64Interp>),
}

impl PointInterp {
    // Pass around some context/ rule thing to control construction if more weird
    // interp options needed
    fn new(old: Point, new: Point) -> ConstOr<PointInterp> {
        if old == new {
            old.into()
        }else {
            PointInterp::Point(
                F64Interp::linear(old.x, new.x),
                F64Interp::linear(old.y, new.y),
            ).wrap()
        }
    }
}

impl Interp for PointInterp {
    type Out = Point;
    fn interp(&self, frac: f64) -> Point {
        match self {
            PointInterp::Point(x, y) => Point::new(x.interp(frac), y.interp(frac)),
        }
    }
}

struct TextMarkInterp {
    txt: String,
    font_fam: FontFamily,
    size: ConstOr<F64Interp>,
    point: ConstOr<PointInterp>,
}

impl TextMarkInterp {
    pub fn new(txt: String, font_fam: FontFamily, size: ConstOr<F64Interp>, point: ConstOr<PointInterp>) -> Self {
        TextMarkInterp {
            txt,
            font_fam,
            size,
            point,
        }
    }
}

impl Interp for TextMarkInterp {
    type Out = TextMark;
    fn interp(&self, frac: f64) -> TextMark {
        TextMark {
            txt: self.txt.clone(),
            font_fam: self.font_fam.clone(),
            size: self.size.interp(frac),
            point: self.point.interp(frac),
        }
    }
}

enum MarkShapeInterp {
    Rect(ConstOr<PointInterp>, ConstOr<PointInterp>),
    Line(ConstOr<PointInterp>, ConstOr<PointInterp>),
    Text(ConstOr<TextMarkInterp>)
}

impl MarkShapeInterp {
    fn new(old: MarkShape, new: MarkShape) -> ConstOr<MarkShapeInterp> {
        fn other_point(r: &Rect) -> Point {
            Point::new(r.x1, r.y1)
        }

        match (old, new) {
            (o, n) if o == n => n.into(),
            (MarkShape::Rect(o), MarkShape::Rect(n)) => MarkShapeInterp::Rect(
                PointInterp::new(o.origin(), n.origin()),
                PointInterp::new(other_point(&o), other_point(&n)),
            ).wrap(),
            (MarkShape::Line(o), MarkShape::Line(n)) => {
                MarkShapeInterp::Line(PointInterp::new(o.p0, n.p0), PointInterp::new(o.p1, n.p1)).wrap()
            }
            (MarkShape::Text(o), MarkShape::Text(n)) => MarkShapeInterp::Text(TextMarkInterp::new(
                n.txt.clone(),
                n.font_fam.clone(),
                F64Interp::linear(o.size, n.size),
                PointInterp::new(o.point, n.point),
            ).wrap()).wrap(),
            (_, n) => n.into(),
        }
    }
}

impl Interp for MarkShapeInterp {
    type Out = MarkShape;

    fn interp(&self, frac: f64) -> MarkShape {
        match self {
            MarkShapeInterp::Rect(o, other) => {
                MarkShape::Rect(Rect::from_points(o.interp(frac), other.interp(frac)))
            }
            MarkShapeInterp::Line(o, other) => {
                MarkShape::Line(Line::new(o.interp(frac), other.interp(frac)))
            }
            MarkShapeInterp::Text(t) => MarkShape::Text(t.interp(frac)),
        }
    }
}

enum ColorInterp {
    Rgba(ConstOr<F64Interp>, ConstOr<F64Interp>, ConstOr<F64Interp>, ConstOr<F64Interp>),
}

impl Interp for ColorInterp {
    type Out = Color;

    fn interp(&self, frac: f64) -> Color {
        match self {
            ColorInterp::Rgba(r, g, b, a) => Color::rgba(
                r.interp(frac),
                g.interp(frac),
                b.interp(frac),
                a.interp(frac),
            ),
        }
    }
}

impl ColorInterp {
    fn new(old: Color, new: Color) -> ColorInterp {
        let (r, g, b, a) = old.as_rgba();
        let (r2, g2, b2, a2) = new.as_rgba();

        ColorInterp::Rgba(
            F64Interp::linear(r, r2),
            F64Interp::linear(g, g2),
            F64Interp::linear(b, b2),
            F64Interp::linear(a, a2),
        )
    }
}

struct MarkInterp {
    id: MarkId,
    shape: ConstOr<MarkShapeInterp>,
    color: ColorInterp,
    hover: Option<Color>,
}

impl MarkInterp {
    pub fn new(id: MarkId, old: Mark, new: Mark) -> Self {
        MarkInterp {
            id,
            shape: MarkShapeInterp::new(old.shape, new.shape),
            color: ColorInterp::new(old.color, new.color),
            hover: new.hover,
        }
    }
}

impl Interp for MarkInterp {
    type Out = Mark;
    fn interp(&self, frac: f64) -> Mark {
        Mark::new(
            self.id,
            self.shape.interp(frac),
            self.color.interp(frac),
            self.hover.clone(),
        )
    }
}

impl Mark {
    pub fn new(id: MarkId, shape: MarkShape, color: Color, hover: Option<Color>) -> Self {
        Mark {
            id,
            shape,
            color,
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

    pub fn enter(&self) -> Self {
        let shape = match &self.shape {
            MarkShape::Rect(r) => MarkShape::Rect(Rect::from_center_size(r.center(), Size::ZERO)),
            MarkShape::Line(l) => {
                let mid = PointInterp::new(l.p0, l.p1).interp(0.5);
                MarkShape::Line(Line::new(mid.clone(), mid))
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

    pub fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        // This should be done as some interpolation of the mark before paint? Maybe
        let color = match (self.id, focus) {
            (id, Some(f)) if id == *f => self.hover.as_ref().unwrap_or(&self.color),
            _ => &self.color,
        };
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

                let tl = ctx
                    .text()
                    .new_text_layout(&t.txt)
                    .font(t.font_fam.clone(), t.size)
                    .text_color(color.clone())
                    .build()
                    .unwrap();
                ctx.with_save(|ctx| {
                    // Flip the coordinates back to draw text
                    ctx.transform(
                        Affine::translate(
                            t.point.to_vec2()
                                - Vec2::new(tl.size().width / 2., 0.0 /*-tl.size().height */),
                        ) * Affine::FLIP_Y,
                    );
                    ctx.draw_text(&tl, Point::ORIGIN);
                });
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

#[derive(Debug, Data, Clone, PartialEq)]
pub enum MarkShape {
    Rect(Rect),
    Line(Line),
    Text(TextMark),
}

// A data series
#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct SeriesId(pub usize);

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct DatumId {
    pub series: SeriesId,
    pub idx: LogIdx,
}

impl DatumId {
    pub fn new(series: SeriesId, idx: LogIdx) -> Self {
        DatumId { series, idx }
    }
} // Series, Generator etc

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct AxisName(pub &'static str);

impl Data for AxisName {
    fn same(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum TickLocator {
    Ordinal(usize),
    F64Bits(u64),
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PlainMarkId {
    Datum(DatumId),
    AxisDomain(AxisName),
    Tick(AxisName, TickLocator),
    TickText(AxisName, TickLocator),
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum MarkId {
    Transition {
        old: Option<PlainMarkId>,
        new: Option<PlainMarkId>,
    },
    Plain(PlainMarkId),
    Unknown,
}

#[derive(Clone)]
pub struct DrawableAxis {
    marks: Vec<Mark>,
}

impl DrawableAxis {
    pub fn new(marks: Vec<Mark>) -> Self {
        DrawableAxis { marks }
    }
}

struct DrawableAxisInterp {
    mark_interp: ConstOr<VecInterp<MarkInterp>>,
}

impl Interp for DrawableAxisInterp {
    type Out = DrawableAxis;
    fn interp(&self, frac: f64) -> DrawableAxis {
        DrawableAxis::new(self.mark_interp.interp(frac))
    }
}

impl DrawableAxisInterp {
    fn new(id_mapper: &impl MarkIdMapper, old: &DrawableAxis, new: &DrawableAxis) -> Self {
        Self {
            mark_interp: VecInterp::new(VisMarksInterp::make_mark_interps(
                id_mapper, &old.marks, &new.marks,
            )),
        }
    }
}

#[derive(Debug)]
pub enum VisEvent {
    MouseEnter(MarkId),
    MouseOut(MarkId),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum DataAge {
    Old,
    New,
}

pub trait MarkIdMapper {
    fn map_id(&self, age: DataAge, id: PlainMarkId) -> MarkId;
}

pub trait Visualization {
    type Input: Data;
    type State: Default + Data + Debug;
    type Layout;
    type IdMapper: MarkIdMapper;

    fn layout(&self, data: &Self::Input, size: Size) -> Self::Layout;
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
    fn data_marks(&self, data: &Self::Input, layout: &Self::Layout) -> Vec<Mark>;

    fn id_mapper(&self, old_data: &Self::Input, data: &Self::Input) -> Self::IdMapper;
}

#[derive(Clone)]
struct VisMarks {
    layout: Vec<DrawableAxis>,
    state: Vec<Mark>,
    data: Vec<Mark>,
}

impl VisMarks {
    fn find_mark(&mut self, pos: Point) -> Option<&mut Mark> {
        self.data
            .iter_mut()
            .chain(self.state.iter_mut())
            .chain(self.layout.iter_mut().flat_map(|a| a.marks.iter_mut()))
            .filter(|mark| mark.hit(pos))
            .next()
    }

    fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        self.data.iter().for_each(|x| x.paint(ctx, focus));
        for axis in self.layout.iter() {
            axis.marks.iter().for_each(|x| x.paint(ctx, focus));
        }
        self.state.iter().for_each(|x| x.paint(ctx, focus));
    }
}

struct VisMarksInterp {
    layout: ConstOr<VecInterp<DrawableAxisInterp>>,
    state: ConstOr<VecInterp<MarkInterp>>,
    data: ConstOr<VecInterp<MarkInterp>>,
}

impl VisMarksInterp {
    fn make_mark_interps(
        id_mapper: &impl MarkIdMapper,
        old: &Vec<Mark>,
        new: &Vec<Mark>,
    ) -> Vec<MarkInterp> {
        let mut matched_marks: HashMap<MarkId, (Option<Mark>, Option<Mark>)> = HashMap::new();
        for s in old.iter() {
            if let MarkId::Plain(p) = s.id {
                matched_marks
                    .entry(id_mapper.map_id(DataAge::Old, p))
                    .or_insert_with(|| (Some(s.clone()), None));
            }
        }

        for e in new.iter().filter(|m| m.id != MarkId::Unknown) {
            if let  MarkId::Plain(p) = e.id {
                matched_marks
                    .entry(id_mapper.map_id(DataAge::New, p))
                    .or_insert_with(|| (None, None))
                    .1 = Some(e.clone())
            }
        }

        matched_marks
            .into_iter()
            .flat_map(|(k, v)| match v {
                (Some(o), Some(n)) => Some(MarkInterp::new(k, o, n)),
                (None, Some(n)) => Some(MarkInterp::new(k, n.enter(), n)),
                (Some(o), None) => {
                    let e = o.enter();
                    Some(MarkInterp::new(k, o, e))
                }
                _ => None,
            })
            .collect()
    }

    fn make_axis_interps(
        id_mapper: &impl MarkIdMapper,
        old: &Vec<DrawableAxis>,
        new: &Vec<DrawableAxis>,
    ) -> Vec<DrawableAxisInterp> {
        // TODO: should match them up by AxisId and handle enter/exit
        old.iter()
            .zip(new.iter())
            .map(|(o, n)| DrawableAxisInterp::new(id_mapper, o, n))
            .collect()
    }

    fn new(id_mapper: &impl MarkIdMapper, old: &VisMarks, new: &VisMarks) -> Self {
        VisMarksInterp {
            layout: VecInterp::new(Self::make_axis_interps(id_mapper, &old.layout, &new.layout)),
            state: VecInterp::new(Self::make_mark_interps(id_mapper, &old.state, &new.state)),
            data: VecInterp::new(Self::make_mark_interps(id_mapper, &old.data, &new.data)),
        }
    }
}

impl Interp for VisMarksInterp {
    type Out = VisMarks;

    fn interp(&self, frac: f64) -> VisMarks {
        VisMarks {
            layout: self.layout.interp(frac),
            state: self.state.interp(frac),
            data: self.data.interp(frac),
        }
    }

    fn wrap(self) -> ConstOr<Self> {
        if self.data.is_const() && self.state.is_const() && self.data.is_const() {
            VisMarks {
                layout: self.layout.const_val().unwrap(),
                state: self.state.const_val().unwrap(),
                data: self.data.const_val().unwrap(),
            }.into()
        }else {
            ConstOr::Interp(self)
        }
    }
}

struct VisTransition {
    // matched_marks: HashMap<MarkId, MarkInterp>,
    cur_nanos: u64,
    end_nanos: u64,
    interp: ConstOr<VisMarksInterp>,
    current: VisMarks,
}

impl VisTransition {
    fn advance(&mut self, nanos: u64) -> bool {
        self.cur_nanos += nanos;
        let frac = (self.cur_nanos as f64) / (self.end_nanos as f64);
        self.current = self.interp.interp(frac);
        self.cur_nanos >= self.end_nanos
    }
}

struct VisInner<VP: Visualization> {
    layout: VP::Layout,
    state: VP::State,
    marks: VisMarks,
    transition: Option<VisTransition>,
    transform: Affine,
    focus: Option<MarkId>,
    phantom_vp: PhantomData<VP>,
}

impl<VP: Visualization> VisInner<VP> {
    pub fn new(
        layout: VP::Layout,
        state: VP::State,
        state_marks: Vec<Mark>,
        data_marks: Vec<Mark>,
        layout_marks: Vec<DrawableAxis>,
        transform: Affine,
    ) -> Self {
        VisInner {
            layout,
            state,
            marks: VisMarks {
                state: state_marks,
                data: data_marks,
                layout: layout_marks,
            },
            transition: None,
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
    pub fn new(visual: V) -> Self {
        Vis {
            visual,
            inner: None,
        }
    }

    fn ensure_state(&mut self, data: &V::Input, size: Size) -> &mut VisInner<V> {
        if self.inner.is_none() {
            let state: V::State = Default::default();

            let layout = self.visual.layout(data, size);
            let state_marks = self.visual.state_marks(data, &layout, &state);
            let data_marks = self.visual.data_marks(data, &layout);
            let layout_marks = self.visual.layout_marks(&layout);

            self.inner = Some(VisInner::new(
                layout,
                state,
                state_marks,
                data_marks,
                layout_marks,
                Affine::FLIP_Y * Affine::translate(Vec2::new(0., -size.height)),
            ));
        }
        self.inner.as_mut().unwrap()
    }
}

impl<VP: Visualization> Widget<VP::Input> for Vis<VP> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut VP::Input, env: &Env) {
        self.ensure_state(data, ctx.size());
        let inner = self.inner.as_mut().unwrap();
        let old_state: VP::State = inner.state.clone();

        match event {
            Event::MouseMove(me) => {
                if let Some(mark) = inner.marks.find_mark(inner.transform.inverse() * me.pos) {
                    match mark.id {
                        MarkId::Unknown => {}
                        _ => {
                            let mi = mark.id.clone();
                            if inner.focus != Some(mi) {
                                self.visual.event(
                                    data,
                                    &inner.layout,
                                    &mut inner.state,
                                    &VisEvent::MouseEnter(mi),
                                );
                                inner.focus = Some(mi);
                                ctx.request_paint();
                            }
                        }
                    }
                } else {
                    if let Some(focus) = inner.focus {
                        self.visual.event(
                            data,
                            &inner.layout,
                            &mut inner.state,
                            &VisEvent::MouseOut(focus),
                        );
                    }
                    if inner.focus.is_some() {
                        inner.focus = None;
                        ctx.request_paint()
                    }
                }
            }
            _ => {}
        }

        if !old_state.same(&inner.state) {
            inner.marks.state = self
                .visual
                .state_marks(data, &inner.layout, &mut inner.state);
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &VP::Input,
        env: &Env,
    ) {
        if let (
            LifeCycle::AnimFrame(nanos),
            Some(VisInner {
                transition, ..
            }),
        ) = (event, &mut self.inner)
        {
            let done = if let Some(transit) = transition {
                transit.advance(*nanos)
            } else {
                true
            };

            if done {
                *transition = None;
            } else {
                ctx.request_anim_frame();
            }
            ctx.request_paint()
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &VP::Input, data: &VP::Input, env: &Env) {
        if !data.same(old_data) {
            if let Some(inner) = &mut self.inner {
                inner.layout = self.visual.layout(data, ctx.size());
                let current = inner.marks.clone();

                inner.marks.layout = self.visual.layout_marks(&inner.layout);
                inner.marks.state = self.visual.state_marks(data, &inner.layout, &inner.state);
                inner.marks.data = self.visual.data_marks(data, &inner.layout);

                let id_mapper = self.visual.id_mapper(old_data, data);

                inner.transition = Some(VisTransition {
                    cur_nanos: 0,
                    end_nanos: 250 * 1_000_000,
                    interp: VisMarksInterp::new(&id_mapper, &current, &inner.marks).wrap(),
                    current,
                });
            }
            ctx.request_anim_frame();
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &VP::Input,
        env: &Env,
    ) -> Size {
        self.inner = None;
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &VP::Input, env: &Env) {
        let size = ctx.size();

        let state = self.ensure_state(data, size);
        ctx.with_save(|ctx| {
            ctx.transform(state.transform);
            if let Some(transit) = &state.transition {
                transit.current.paint(ctx, &state.focus);
            } else {
                let marks = &state.marks;
                marks.paint(ctx, &state.focus);
            }
        });
    }
}

pub struct BandScale<T: Clone + Ord + Hash + Display> {
    name: AxisName,
    range: F64Range,
    bands: Vec<T>,
    bands_lookup: HashMap<T, usize>,
    range_per_band: f64,
    half_padding: f64,
}

impl<T: Clone + Ord + Hash + Display> BandScale<T> {
    pub fn new(
        name: AxisName,
        range: F64Range,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64,
    ) -> Self {
        let mut uniq = BTreeSet::new();
        for item in bands_it {
            uniq.insert(item);
        }
        let bands: Vec<T> = uniq.iter().cloned().collect();
        let bands_lookup: HashMap<T, usize> =
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
        let idx = self.bands_lookup.get(domain_val).unwrap();
        let start = self.range.0 + ((*idx as f64) * self.range_per_band);
        F64Range(
            start + self.half_padding,
            start + self.range_per_band - self.half_padding,
        )
    }

    pub fn make_axis(&self) -> DrawableAxis {
        let mut marks = Vec::new();
        let line_y = 30.0;
        let tick_extent = line_y - 8.;
        let label_top = tick_extent - 2.;
        marks.push(Mark::new(
            MarkId::Plain(PlainMarkId::AxisDomain(self.name)),
            MarkShape::Line(Line::new((self.range.0, line_y), (self.range.1, line_y))),
            Color::WHITE,
            None,
        ));
        for (i, v) in self.bands.iter().enumerate() {
            let b_mid = self.range_val(v).mid();
            marks.push(Mark::new(
                 // TODO: if the domain is changing the id_mapper needs to fiddle with these
                 MarkId::Plain(PlainMarkId::Tick(self.name, TickLocator::Ordinal(i))),
                MarkShape::Line(Line::new((b_mid, tick_extent), (b_mid, line_y))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::Plain(PlainMarkId::TickText(self.name, TickLocator::Ordinal(i))),
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
        DrawableAxis::new(
            //Axis::Horizontal,
            //CrossAxisAlignment::Start,
            marks,
            //Size::new(self.range.1 - self.range.0, line_y),
        )
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
        let (start, stop) = domain_iter.minmax().into_option().unwrap_or_else(||(Default::default(), Default::default()));
        let domain_range = F64Range(start.as_f64(), stop.as_f64()).include_zero(zero);
        let (domain_range, tick_step) = if nice {
            domain_range.nice(ticks)
        } else {
            (domain_range, domain_range.step_size(ticks))
        };
        let domain_dist = domain_range.distance();

        let ticks = (domain_dist / tick_step).ceil() as usize;
        let multiplier = if domain_dist == 0.0 { 0.0 } else { range.distance() / domain_dist };
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
            MarkId::Plain(PlainMarkId::AxisDomain(self.name)),
            MarkShape::Line(Line::new((line_x, self.range.0), (line_x, self.range.1))),
            Color::WHITE,
            None,
        ));

        for step in 0..=self.ticks {
            let d_v = self.domain_range.0 + self.tick_step * (step as f64);
            let value = T::from_f64(d_v);

            let r_v = self.range_val_raw(d_v);
            marks.push(Mark::new(
                MarkId::Plain(PlainMarkId::Tick(self.name, TickLocator::F64Bits(d_v.to_bits()))),
                MarkShape::Line(Line::new((tick_extent, r_v), (line_x, r_v))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::Plain(PlainMarkId::TickText(self.name, TickLocator::F64Bits(d_v.to_bits()))),
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
