use crate::LogIdx;
use druid::kurbo::{
    Affine, Line, ParamCurveNearest, Point, Rect, Size, Vec2,
};
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
use crate::vis::InterpError::{ValueMismatch, IndexOutOfBounds};
use std::ops::IndexMut;

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

#[derive(Eq, PartialEq, Debug)]
enum InterpError{
    NoDefaultForNoop,
    FracOutOfBounds,
    ValueMismatch,
    IndexOutOfBounds,
    Multiple
}

type InterpResult = Result<(), InterpError>;
const OK : InterpResult = Ok(());

trait Interp: Sized {
    type Value: Clone + Debug;
    fn interp(&self, frac: Frac, val: &mut Self::Value)->InterpResult;
    fn wrap(self)->ConstOr<Self>{
        ConstOr::Interp(self)
    }

    fn interp_default(&self, frac: f64)->Result<Self::Value, InterpError> where Self::Value : Default {
        let mut temp: Self::Value = Default::default();
        self.interp(frac, &mut temp).map(move |_| temp)
    }
}

#[derive(Debug)]
enum ConstOr<P: Interp> {
    Const(P::Value),
    Noop,
    Interp(P)
}

impl <P: Interp> ConstOr<P>{

    fn is_noop(&self) ->bool{
        matches!(self, ConstOr::Noop)
    }
}

impl <T: Clone + Debug, P: Interp<Value=T>> From<T> for ConstOr<P> {
    fn from(t: P::Value) -> Self {
        ConstOr::Const(t)
    }
}

impl<P: Interp> Interp for ConstOr<P>{
    type Value = P::Value;
    fn interp(&self, frac: f64, val: &mut Self::Value)->InterpResult {
        match self {
            ConstOr::Const(t) => {
                *val = t.clone();
                OK
            },
            ConstOr::Noop => OK,
            ConstOr::Interp(i) => i.interp(frac, val)
        }
    }

    fn interp_default(&self, frac: f64) -> Result<Self::Value, InterpError> where Self::Value: Default {
        match self{
            ConstOr::Noop => Err(InterpError::NoDefaultForNoop),
            _=>{
                let mut temp: Self::Value = Default::default();
                self.interp(frac, &mut temp).map(move |_| temp)
            }
        }
    }
}

#[derive(Debug)]
struct VecInterp<TInterp: Interp> {
    interps: Vec<(usize, ConstOr<TInterp>)>
}

impl <TInterp: Interp> VecInterp<TInterp> {
    pub fn new(interps_iter: impl Iterator<Item=ConstOr<TInterp>>) -> ConstOr<Self> where TInterp::Value: Clone {
        let mut all_noop = true;
        let mut interps = Vec::new();
        for (idx, interp) in interps_iter.enumerate(){
            if !interp.is_noop(){
                all_noop = false;
                interps.push( (idx, interp))
            }
        }

        if all_noop{
            ConstOr::Noop
        }else {
            VecInterp {
                interps
            }.wrap()
        }
    }
}

impl <TInterp: Interp> Interp for VecInterp<TInterp> {
    type Value = Vec<TInterp::Value>;
    fn interp(&self, frac: f64, val: &mut Vec<TInterp::Value>)->InterpResult {
        let mut loop_err: Option<InterpError> = None;
        for (idx, interp) in self.interps.iter() {
            let cur_err = val.get_mut( *idx ).map(|value|{
                interp.interp(frac, value)
            }).unwrap_or(Err(IndexOutOfBounds));

            match (&mut loop_err, cur_err) {
                (loop_err @None, Err(c_e))=> *loop_err = Some(c_e),
                (Some(l_e), Err(c_e)) if *l_e != c_e => *l_e = InterpError::Multiple,
                _=>()
            }
        }
        loop_err.map(Err).unwrap_or(OK)
    }
}

#[derive(Debug)]
enum F64Interp {
    Linear(f64, f64),
}

impl F64Interp{
    fn linear(start: f64, end: f64, noop: bool)->ConstOr<F64Interp>{
        if start == end{
            if noop {
                ConstOr::Noop
            }else {
                start.into()
            }
        }else{
            F64Interp::Linear(start, end).wrap()
        }
    }
}

impl Interp for F64Interp {
    type Value = f64;
    fn interp(&self, frac: f64, val: &mut f64) -> InterpResult{
        if frac < 0.0 || frac > 1.0 {
            Err(InterpError::FracOutOfBounds)
        }else {
            match self {
                F64Interp::Linear(start, end) => *val = start + (end - start) * frac,
            }
            OK
        }
    }
}

#[derive(Debug)]
enum PointInterp {
    Point(ConstOr<F64Interp>, ConstOr<F64Interp>),
}

impl PointInterp {
    // Pass around some context/ rule thing to control construction if more weird
    // interp options needed?
    fn new(old: Point, new: Point) -> ConstOr<PointInterp> {
        if old == new {
            ConstOr::Noop
        }else {
            PointInterp::Point(
                F64Interp::linear(old.x, new.x, true),
                F64Interp::linear(old.y, new.y, true),
            ).wrap()
        }
    }
}

impl Interp for PointInterp {
    type Value = Point;
    fn interp(&self, frac: f64, val: &mut Point) ->InterpResult{
        match self {
            PointInterp::Point(x, y) => {
                x.interp(frac, &mut val.x)?;
                y.interp(frac, &mut val.y)?;
            }
        }
        OK
    }
}

#[derive(Debug)]
struct StringInterp{
    prefix: String,
    remove: String,
    add: String,
    steps: usize
}

impl StringInterp{
    fn new(a: &str, b: &str)-> ConstOr<StringInterp>{
        let prefix: String = a.chars().zip(b.chars()).take_while(|(a,b)|a==b).map(|v|v.0).collect();
        if prefix.len() == a.len() && prefix.len() == b.len(){
            ConstOr::Noop
        }else{
            let remove: String = a[prefix.len()..].into();
            let add: String = b[prefix.len()..].into();
            let steps = remove.len() + add.len();
            StringInterp{
                prefix,
                remove,
                add,
                steps
            }.wrap()
        }
    }
}

impl Interp for StringInterp{
    type Value = String;

    fn interp(&self, frac: f64, val: &mut Self::Value) -> InterpResult{
        // TODO: do this by modifying? Can't assume that calls are in order though
        let step = ((self.steps as f64) * frac) as isize;
        let r_len = self.remove.len() as isize;
        let step = step - r_len;
        if step > 0 {
            *val = format!("{}{}", self.prefix, &self.add[..(step as usize)]).into()
        }else if step < 0 {
            *val = format!("{}{}", self.prefix, &self.remove[.. (- step) as usize]).into()
        }else{
            *val = self.prefix.clone()
        }
        OK
    }
}

#[derive(Debug)]
struct TextMarkInterp {
    txt: ConstOr<StringInterp>,
    size: ConstOr<F64Interp>,
    point: ConstOr<PointInterp>,
}

impl TextMarkInterp {
    pub fn new(txt: ConstOr<StringInterp>, size: ConstOr<F64Interp>, point: ConstOr<PointInterp>) -> Self {
        TextMarkInterp {
            txt,
            size,
            point,
        }
    }
}

impl Interp for TextMarkInterp {
    type Value = TextMark;
    fn interp(&self, frac: f64, val: &mut TextMark) -> InterpResult{
        self.txt.interp(frac, &mut val.txt)?;
        self.size.interp(frac, &mut val.size)?;
        self.point.interp(frac, &mut val.point)?;
        OK
    }
}

#[derive(Debug)]
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
            (o, n) if o.same(&n) => ConstOr::Noop,
            (MarkShape::Rect(o), MarkShape::Rect(n)) => MarkShapeInterp::Rect(
                PointInterp::new(o.origin(), n.origin()),
                PointInterp::new(other_point(&o), other_point(&n)),
            ).wrap(),
            (MarkShape::Line(o), MarkShape::Line(n)) => {
                MarkShapeInterp::Line(PointInterp::new(o.p0, n.p0), PointInterp::new(o.p1, n.p1)).wrap()
            }
            (MarkShape::Text(o), MarkShape::Text(n)) => MarkShapeInterp::Text(TextMarkInterp::new(
                StringInterp::new(&o.txt, &n.txt),
                F64Interp::linear(o.size, n.size, true),
                PointInterp::new(o.point, n.point),
            ).wrap()).wrap(),
            (_, n) => ConstOr::Noop
        }
    }
}

impl Interp for MarkShapeInterp {
    type Value = MarkShape;

    fn interp(&self, frac: f64, val: &mut MarkShape) -> InterpResult{
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
            (int, val)=>{
                Err(InterpError::ValueMismatch)
            }
        }
    }
}

#[derive(Debug)]
enum ColorInterp {
    Rgba(ConstOr<F64Interp>, ConstOr<F64Interp>, ConstOr<F64Interp>, ConstOr<F64Interp>),
}

impl Interp for ColorInterp {
    type Value = Color;

    fn interp(&self, frac: f64, val: &mut Color) -> InterpResult{
        match self {
            ColorInterp::Rgba(r, g, b, a) => {
                // TODO: mutate this?
                *val = Color::rgba(
                    r.interp_default(frac)?,
                g.interp_default(frac)?,
                b.interp_default(frac)?,
                a.interp_default(frac)?
                );
                OK
            }
        }
    }
}

impl ColorInterp {
    fn new(old: Color, new: Color) -> ConstOr<ColorInterp> {
        if old.same(&new) {
            ConstOr::Noop
        }else{
            let (r, g, b, a) = old.as_rgba();
            let (r2, g2, b2, a2) = new.as_rgba();

            ColorInterp::Rgba(
                F64Interp::linear(r, r2, false),
                F64Interp::linear(g, g2, false),
                F64Interp::linear(b, b2, false),
                F64Interp::linear(a, a2, false),
            ).wrap()
        }
    }
}

#[derive(Debug)]
struct MarkInterp {
    shape: ConstOr<MarkShapeInterp>,
    color: ConstOr<ColorInterp>
}

impl MarkInterp {
    pub fn new(old: Mark, new: Mark) -> ConstOr<Self> {
        if old.same(&new){
            ConstOr::Noop
        }else {
            MarkInterp {
                shape: MarkShapeInterp::new(old.shape, new.shape),
                color: ColorInterp::new(old.color, new.color),
            }.wrap()
        }
    }
}

impl Interp for MarkInterp {
    type Value = Mark;
    fn interp(&self, frac: f64, val: &mut Mark)->InterpResult {
        self.shape.interp(frac, &mut val.shape)?;
        self.color.interp(frac, &mut val.color)?;
        OK
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

    // Todo: enter/exit etc overrides on the mark but maybe at other levels too
    pub fn enter(&self) -> Self {
        let shape = match &self.shape {
            MarkShape::Rect(r) => MarkShape::Rect(Rect::from_center_size(r.center(), Size::ZERO)),
            MarkShape::Line(l) => {
                let mid = PointInterp::new(l.p0, l.p1).interp_default(0.5).unwrap_or( Point::ZERO );
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

    pub fn paint(&self, ctx: &mut PaintCtx, focus: &Option<PlainMarkId>) {
        // This should be done as some interpolation of the mark before paint? Maybe
        let color = if focus.is_some() && self.id.new_plain_id() == *focus {
           self.hover.as_ref().unwrap_or(&self.color)
        }else{
            &self.color
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

#[derive(Debug, Data, Clone)]
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
    Persistent(PersistentId),
    U64Bits(u64),
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PlainMarkId {
    Datum(DatumId),
    AxisDomain(AxisName),
    Tick(AxisName, TickLocator),
    TickText(AxisName, TickLocator),
    StateMark(u32)
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

impl MarkId{
    pub fn new_plain_id(&self)->Option<PlainMarkId>{
        match self{
            MarkId::Plain(p)=>Some(*p),
            MarkId::Transition {new, ..} if new.is_some()=>*new,
            _=>None
        }
    }
}

#[derive(Clone, Debug)]
pub struct DrawableAxis {
    marks: Vec<Mark>,
}

impl Data for DrawableAxis{
    fn same(&self, other: &Self) -> bool {
        self.marks.len() == other.marks.len() && self.marks.iter().zip(other.marks.iter()).all(|(o, n)|o.same(n))
    }
}

impl DrawableAxis {
    pub fn new(marks: Vec<Mark>) -> Self {
        DrawableAxis { marks }
    }
}

#[derive(Debug)]
struct DrawableAxisInterp {
    mark_interp: ConstOr<VecInterp<MarkInterp>>,
}

impl Interp for DrawableAxisInterp {
    type Value = DrawableAxis;
    fn interp(&self, frac: f64, val: &mut DrawableAxis) -> InterpResult{
        self.mark_interp.interp(frac, &mut val.marks)
    }
}

impl DrawableAxisInterp {
    fn new(id_mapper: &impl MarkIdMapper, old: DrawableAxis, new: &DrawableAxis) -> (ConstOr<Self>, DrawableAxis) {
        if old.same(new) {
            (ConstOr::Noop, old)
        } else {
            let (m_ints, marks) =
                VisMarksInterp::make_mark_interps(id_mapper, old.marks, &new.marks);
            (Self {
                mark_interp: VecInterp::new(m_ints.into_iter())
            }.wrap(), DrawableAxis::new(marks))
        }
    }
}

#[derive(Debug)]
pub enum VisEvent {
    MouseEnter(PlainMarkId),
    MouseOut(PlainMarkId),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum DataAge {
    Old,
    New,
}

pub trait MarkIdMapper: Default {
    fn map_id(&self, age: DataAge, id: PlainMarkId) -> MarkId;
}

pub trait Visualization {
    type Input: Data;
    type State: Default + Data + Debug;
    type Layout;
    type IdMapper: MarkIdMapper;

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
    fn data_marks(&self, data: &Self::Input, layout: &Self::Layout) -> Vec<Mark>;

    fn id_mapper(&self, old_data: &Self::Input, data: &Self::Input) -> Self::IdMapper;
}

#[derive(Clone, Debug)]
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

    fn paint(&self, ctx: &mut PaintCtx, focus: &Option<PlainMarkId>) {
        self.data.iter().for_each(|x| x.paint(ctx, focus));
        for axis in self.layout.iter() {
            axis.marks.iter().for_each(|x| x.paint(ctx, focus));
        }
        self.state.iter().for_each(|x| x.paint(ctx, focus));
    }
}

#[derive(Debug)]
struct VisMarksInterp {
    layout: ConstOr<VecInterp<DrawableAxisInterp>>,
    state: ConstOr<VecInterp<MarkInterp>>,
    data: ConstOr<VecInterp<MarkInterp>>,
}

impl VisMarksInterp {
    fn make_mark_interps(
        id_mapper: &impl MarkIdMapper,
        old: Vec<Mark>,
        new: &Vec<Mark>,
    ) -> (Vec<ConstOr<MarkInterp>>, Vec<Mark>) {
        let mut matched_marks: HashMap<MarkId, (Option<Mark>, Option<Mark>)> = HashMap::new();

        let mut exiting = Vec::new();
        for s in old.into_iter() {
            let p_id = match s.id {
                MarkId::Plain(p) => Some(p),
                MarkId::Transition {new, ..} if new.is_some() =>{
                    new
                },
                s_id => {
                    None
                }
            };
            if let Some(p) = p_id{
                matched_marks
                    .entry(id_mapper.map_id(DataAge::Old, p))
                    .or_insert_with(|| (Some(s), None));
            }else{
                exiting.push(s)
            }
        }

        for e in new.iter().filter(|m| m.id != MarkId::Unknown) {
            if let MarkId::Plain(p) = e.id {
                matched_marks
                    .entry(id_mapper.map_id(DataAge::New, p))
                    .or_insert_with(|| (None, None))
                    .1 = Some(e.clone())
            }
        }

        matched_marks
            .into_iter()
            .flat_map(|(k, v)| match v {
                (Some(o), Some(n)) => Some((MarkInterp::new( o.clone(), n), o)),
                (None, Some(n)) => {
                    let e = n.enter();
                    Some((MarkInterp::new( e.clone(), n),  e))
                },
                (Some(o), None) => {
                    let e = o.enter(); // TODO: exit
                    Some((MarkInterp::new( o.clone(), e), o))
                }
                _ => None,
            }).chain(
                exiting.into_iter().map(|o|{
                    let e = o.enter();
                    (MarkInterp::new( o.clone(), e), o)
                })
            )
            .unzip()
    }

    fn make_axis_interps(
        id_mapper: &impl MarkIdMapper,
        old: Vec<DrawableAxis>,
        new: &Vec<DrawableAxis>,
    ) -> (Vec<ConstOr<DrawableAxisInterp>>, Vec<DrawableAxis>) {
        // TODO: should match them up by AxisId and handle enter/exit
        old.into_iter()
            .zip(new.iter())
            .map(|(o, n)| DrawableAxisInterp::new(id_mapper, o, n))
            .unzip()
    }

    fn build(id_mapper: &impl MarkIdMapper, old: VisMarks, new: &VisMarks) -> (ConstOr<Self>, VisMarks) {
        let VisMarks{layout, state, data} = old;

        let (l_ints, layout)= Self::make_axis_interps(id_mapper, layout, &new.layout);
        let (s_ints, state) = Self::make_mark_interps(id_mapper, state, &new.state);
        let (d_ints, data) = Self::make_mark_interps(id_mapper, data, &new.data);

        let vis_marks = VisMarks{
            layout, state, data
        };

        let (layout, state, data) = (
            VecInterp::new(l_ints.into_iter()),
            VecInterp::new(s_ints.into_iter()),
            VecInterp::new(d_ints.into_iter()),
        );

        let vmi = if layout.is_noop() && state.is_noop() && data.is_noop() {
            ConstOr::Noop
        } else {
           VisMarksInterp { layout, state, data }.wrap()
        };

        (vmi, vis_marks)
    }
}

impl Interp for VisMarksInterp {
    type Value = VisMarks;

    fn interp(&self, frac: f64, val: &mut VisMarks) -> InterpResult{
        self.layout.interp(frac, &mut val.layout)?;
        self.state.interp(frac, &mut val.state)?;
        self.data.interp(frac, &mut val.data)
    }

}

struct VisTransition {
    // matched_marks: HashMap<MarkId, MarkInterp>,
    cur_nanos: u64,
    end_nanos: u64,
    interp: ConstOr<VisMarksInterp>,
    current: VisMarks
}

impl VisTransition {
    fn advance(&mut self, nanos: u64) -> bool {
        self.cur_nanos += nanos;
        let frac = (self.cur_nanos as f64) / (self.end_nanos as f64);
        self.interp.interp(frac, &mut self.current);
        self.cur_nanos >= self.end_nanos
    }
}

struct VisInner<VP: Visualization> {
    layout: VP::Layout,
    state: VP::State,
    marks: VisMarks,
    transition: Option<VisTransition>,
    transform: Affine,
    focus: Option<PlainMarkId>,
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

    fn start_transition(&mut self, size: Size, data: &V::Input, id_mapper: V::IdMapper) {
        if let Some(inner) = &mut self.inner {

            inner.layout = self.visual.layout(data, size);

            let mut temp = VisMarks {
                layout: self.visual.layout_marks(&inner.layout),
                state: self.visual.state_marks(data, &inner.layout, &inner.state),
                data: self.visual.data_marks(data, &inner.layout)
            };
            std::mem::swap(&mut inner.marks, &mut temp);

            let start = if let Some(old_transit) = inner.transition.take() {
                old_transit.current
            }else{
                temp
            };
            let (interp, current) = VisMarksInterp::build(&id_mapper, start, &inner.marks);
            log::info!("Interp: {:#?}", interp);

            inner.transition = if interp.is_noop(){
                None
            } else {
                Some(VisTransition {
                    cur_nanos: 0,
                    end_nanos: 250 * 1_000_000,
                    interp,
                    current
                })
            };
        }
    }

}

impl<V: Visualization> Widget<V::Input> for Vis<V> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut V::Input, env: &Env) {
        self.ensure_state(data, ctx.size());
        let inner = self.inner.as_mut().unwrap();
        let old_state: V::State = inner.state.clone();

        match event {
            Event::MouseMove(me) => {
                if let Some(mark) = inner.marks.find_mark(inner.transform.inverse() * me.pos) {
                    if let Some(mi) = mark.id.new_plain_id() {
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
            self.start_transition(ctx.size(), data, V::IdMapper::default());
            ctx.request_anim_frame();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &V::Input,
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

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &V::Input, data: &V::Input, env: &Env) {
        if !data.same(old_data) {
            self.start_transition(ctx.size(), data, self.visual.id_mapper(old_data, data));
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

type PersistentId = usize;

pub struct BandScaleFactory<T> {
    name: AxisName,
    cat_to_id: HashMap<T, PersistentId>
}

impl<T: Clone + Ord + Hash + Display> BandScaleFactory<T> {
    pub fn new(name: AxisName) -> Self {
        BandScaleFactory { name, cat_to_id: Default::default() }
    }

    pub fn make_scale(
        &mut self,
        range: F64Range,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64
    )->BandScale<T>{
        BandScale::new(self.name, range, |cat_item| self.get_id(cat_item)
        , bands_it, padding_ratio)
    }

    pub fn get_id(&mut self, item: &T)->PersistentId{
        let next = self.cat_to_id.len();
        self.cat_to_id.entry(item.clone()).or_insert(next).clone()
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
        mut get_persistent_id: impl FnMut(&T)->PersistentId,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64,
    ) -> Self {
        let mut uniq = BTreeSet::new();
        for item in bands_it {
            uniq.insert(item);
        }
        let bands: Vec<_> = uniq.iter().map(|band|{
            let persistent_id = get_persistent_id(&band);
            (band.clone(), persistent_id)
        }).collect();
        let bands_lookup: HashMap<_, _> =
            uniq.into_iter().enumerate()
                .map(|(i, v)|(v, i))
                .collect();
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
        }else{
            F64Range(0., 0.) // todo : propagate option?
        }
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
        for (v, p_id) in self.bands.iter() {
            let tick_loc = TickLocator::Persistent(*p_id);
            let b_mid = self.range_val(v).mid();
            marks.push(Mark::new(
                 MarkId::Plain(PlainMarkId::Tick(self.name, tick_loc)),
                MarkShape::Line(Line::new((b_mid, tick_extent), (b_mid, line_y))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::Plain(PlainMarkId::TickText(self.name, tick_loc)),
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
            marks,
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
                MarkId::Plain(PlainMarkId::Tick(self.name, TickLocator::U64Bits(d_v.to_bits()))),
                MarkShape::Line(Line::new((tick_extent, r_v), (line_x, r_v))),
                Color::WHITE,
                None,
            ));
            marks.push(Mark::new(
                MarkId::Plain(PlainMarkId::TickText(self.name, TickLocator::U64Bits(d_v.to_bits()))),
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


