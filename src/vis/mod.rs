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
use InterpError::*;

// Could new type with bounds check
// Number between 0 and 1
struct Frac<'a>{
    cur_idx: usize,
    fracs: &'a [f64]
}

impl Frac<'_> {
    pub fn new(cur_idx: usize, fracs: &[f64]) -> Frac {
        if cur_idx >= fracs.len(){
            panic!("frac index out of range")
        }
        Frac { cur_idx, fracs }
    }

    pub fn current(&self)->f64{
        self.fracs[self.cur_idx]
    }
}

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
    FracOutOfBounds,
    ValueMismatch,
    IndexOutOfBounds,
    Multiple
}

type InterpResult = Result<(), InterpError>;
const OK : InterpResult = Ok(());

trait Interp: Sized {
    type Value: HasInterp<Interp=Self>;
    fn interp(&self, frac: &Frac, val: &mut Self::Value)->InterpResult;

    fn pod(self) -> Pod<Self>{
        if self.is_noop(){
            Pod::Noop
        }else {
            Pod::Interp(self)
        }
    }

    fn is_noop(&self)->bool;

    fn leaf()->bool{
        false
    }

    fn select_anim(self, idx: usize)->Self;

    fn merge(self, other: Self)->Self;
}

trait HasInterp: Clone + Debug{
    type Interp : Interp<Value=Self>;
}

#[derive(Debug)]
enum Pod<P: Interp> {
    Noop,
    Interp(P),
    SelectAnim(usize, P)
}


impl <P: Interp> Pod<P>{
    fn is_noop(&self) ->bool{
        matches!(self, Pod::Noop)
    }

    fn interp(&self, frac: &Frac, val: &mut P::Value)->InterpResult {
        match self {
            Pod::Noop => OK,
            Pod::Interp(interp) => interp.interp(frac, val),
            Pod::SelectAnim(cur_idx, interp) => {
                let new_frac = Frac{
                    cur_idx:*cur_idx,
                    fracs: frac.fracs
                };
                interp.interp(&new_frac, val)
            }
        }
    }

    fn select_anim(self, idx: usize)->Self {
        match self{
            Pod::Interp(interp) => Pod::SelectAnim(idx, interp),
            s=>s
        }
    }

    fn merge(self, other: Pod<P>) ->Self{
        fn wrap_anim<P: Interp>(p: P, idx: usize)-> Pod<P>{
            if P::leaf() { Pod::SelectAnim(idx, p) } else { Pod::Interp(p) }
        }
        match (self, other){
            (Pod::Noop, other)=>other,
            (other, Pod::Noop)=>other,
            //(ConstOr::Const(t1), ConstOr::Const(t2)) if t1 == t2 => ConstOr::Const(t1),
            (Pod::SelectAnim(a1, i1), Pod::SelectAnim(a2, i2))=> {
                wrap_anim(i1.select_anim(a1).merge(i2.select_anim(a2)), a2)
            },
            (Pod::SelectAnim(a1, i1), Pod::Interp(p))=> {
                wrap_anim(i1.select_anim(a1).merge(p), a1)
            },
            (Pod::Interp(p), Pod::SelectAnim(a2, i2))=>{
                wrap_anim(p.merge( i2.select_anim(a2)), a2)
            },
            (_, other)=>other
        }
    }
}

#[derive(Debug)]
struct MapInterp<TInterp: Interp, Key: Hash + Eq> {
    interps: Vec<(Key, Pod<TInterp>)>
}

impl <TInterp: Interp, Key: Hash + Eq + Clone + Debug> MapInterp<TInterp, Key> {
    pub fn new(interps_iter: impl Iterator<Item=(Key, Pod<TInterp>)>) -> Pod<Self> where TInterp::Value: Clone {
        let mut all_noop = true;
        let mut interps = Vec::new();
        for (key, interp) in interps_iter{
            if !interp.is_noop(){
                all_noop = false;
                interps.push( (key, interp))
            }
        }

        if all_noop{
            Pod::Noop
        }else {
            MapInterp {
                interps
            }.pod()
        }
    }
}

impl <HI: HasInterp, K: Eq + Hash + Clone + Debug> HasInterp for HashMap<K, HI> {
    type Interp = MapInterp<HI::Interp, K>;
}

impl <TInterp: Interp, Key: Debug + Hash + Eq + Clone> Interp for MapInterp<TInterp, Key> {
    type Value = HashMap<Key, TInterp::Value>;
    fn interp(&self, frac: &Frac, val: &mut HashMap<Key, TInterp::Value>)->InterpResult {
        let mut loop_err: Option<InterpError> = None;
        for (key, interp) in self.interps.iter() {
            let cur_err = val.get_mut( key ).map(|value|{
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

    fn is_noop(&self) -> bool {
        self.interps.is_empty()
    }

    fn select_anim(self, a_idx: usize) ->Self{
        MapInterp {
            interps: self.interps.into_iter().map(|(key, interp)| (key, interp.select_anim(a_idx))).collect()
        }
    }

    fn merge(self, other: MapInterp<TInterp, Key>) -> Self {
        let mut interps : HashMap<_, _> = self.interps.into_iter().collect();
        for (key, interp) in other.interps.into_iter(){
            let new_interp = if let Some(cur) = interps.remove(&key){
                cur.merge(interp)
            }else{
                interp
            };
            if !new_interp.is_noop() {
                interps.insert(key, new_interp);
            }
        }

        MapInterp {
            interps: interps.into_iter().collect()
        }
    }
}

#[derive(Debug)]
struct F64Interp {
    start: f64,
    end: f64
}

impl F64Interp{
    fn new(start: f64, end: f64)-> F64Interp{
        Self{start, end}
    }

    fn interp_raw(start: f64, end: f64, frac: f64)->f64{
        start + (end - start) * frac
    }
}

impl HasInterp for f64{
    type Interp = F64Interp;
}

impl Interp for F64Interp {
    type Value = f64;
    fn interp(&self, frac: &Frac, val: &mut f64) -> InterpResult{
        *val = Self::interp_raw(self.start, self.end, frac.current());
        OK
    }

    fn is_noop(&self) -> bool {
        self.start == self.end
    }

    fn leaf() -> bool {
        true
    }

    fn select_anim(self, idx: usize) -> Self {
        self
    }

    fn merge(self, other: Self) -> Self {
        other
    }
}

#[derive(Debug)]
struct PointInterp {
    x: Pod<F64Interp>,
    y: Pod<F64Interp>
}


impl PointInterp {
    // Pass around some context/ rule thing to control construction if more weird
    // interp options needed?
    fn new(old: Point, new: Point) -> PointInterp {
        PointInterp {
            x: F64Interp::new(old.x, new.x).pod(),
            y: F64Interp::new(old.y, new.y).pod(),
        }
    }
}

impl HasInterp for Point{
    type Interp = PointInterp;
}

impl Interp for PointInterp {
    type Value = Point;
    fn interp(&self, frac: &Frac, val: &mut Point) ->InterpResult{
        self.x.interp(frac, &mut val.x)?;
        self.y.interp(frac, &mut val.y)
    }

    fn is_noop(&self) -> bool {
        self.x.is_noop() && self.y.is_noop()
    }


    fn select_anim(self, idx: usize) -> Self {
        Self {
            x: self.x.select_anim(idx),
            y: self.y.select_anim(idx)
        }
    }

    fn merge(self, other: Self) -> Self {
        PointInterp {
            x: self.x.merge(other.x),
            y: self.y.merge(other.y),
        }
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
    fn new(a: &str, b: &str)-> StringInterp {
        let prefix: String = a.chars().zip(b.chars()).take_while(|(a,b)|a==b).map(|v|v.0).collect();
        let remove: String = a[prefix.len()..].into();
        let add: String = b[prefix.len()..].into();
        let steps = remove.len() + add.len();
        StringInterp{
                prefix,
                remove,
                add,
                steps
        }
    }
}

impl HasInterp for String{
    type Interp = StringInterp;
}

impl Interp for StringInterp{
    type Value = String;

    fn interp(&self, frac: &Frac, val: &mut Self::Value) -> InterpResult{
        // TODO: do this by modifying? Can't assume that calls are in order though
        let step = ((self.steps as f64) * frac.current() ) as isize;
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

    fn is_noop(&self) -> bool {
        self.steps == 0
    }

    fn select_anim(self, idx: usize) -> Self {
        self
    }

    fn merge(self, other: Self) -> Self {
        other
    }
}

#[derive(Debug)]
struct TextMarkInterp {
    txt: Pod<StringInterp>,
    size: Pod<F64Interp>,
    point: Pod<PointInterp>,
}

impl TextMarkInterp {
    pub fn new(txt: Pod<StringInterp>, size: Pod<F64Interp>, point: Pod<PointInterp>) -> Self {
        TextMarkInterp {
            txt,
            size,
            point,
        }
    }
}

impl HasInterp for TextMark{
    type Interp = TextMarkInterp;
}

impl Interp for TextMarkInterp {
    type Value = TextMark;
    fn interp(&self, frac: &Frac, val: &mut TextMark) -> InterpResult{
        self.txt.interp(frac, &mut val.txt)?;
        self.size.interp(frac, &mut val.size)?;
        self.point.interp(frac, &mut val.point)?;
        OK
    }

    fn is_noop(&self) -> bool {
        self.point.is_noop() && self.size.is_noop() && self.txt.is_noop()
    }

    fn select_anim(self, idx: usize) -> Self {
        let TextMarkInterp{
            txt, size, point
        } = self;
        TextMarkInterp{
            txt: txt.select_anim(idx),
            size: size.select_anim(idx),
            point: point.select_anim(idx)
        }
    }

    fn merge(self, other: Self) -> Self {
        let TextMarkInterp{
            txt, size, point
        } = self;
        let TextMarkInterp{
            txt:txt2, size:size2, point:point2
        } = other;
        TextMarkInterp{
            txt: txt.merge(txt2),
            size: size.merge(size2),
            point: point.merge(point2)
        }
    }
}

#[derive(Debug)]
enum MarkShapeInterp {
    Rect(Pod<PointInterp>, Pod<PointInterp>),
    Line(Pod<PointInterp>, Pod<PointInterp>),
    Text(Pod<TextMarkInterp>),
    Noop
}

impl MarkShapeInterp {
    fn new(old: MarkShape, new: MarkShape) -> MarkShapeInterp {
        fn other_point(r: &Rect) -> Point {
            Point::new(r.x1, r.y1)
        }

        match (old, new) {
            (o, n) if o.same(&n) => MarkShapeInterp::Noop,
            (MarkShape::Rect(o), MarkShape::Rect(n)) => MarkShapeInterp::Rect(
                PointInterp::new(o.origin(), n.origin()).pod(),
                PointInterp::new(other_point(&o), other_point(&n)).pod(),
            ),
            (MarkShape::Line(o), MarkShape::Line(n)) => {
                MarkShapeInterp::Line(PointInterp::new(o.p0, n.p0).pod(), PointInterp::new(o.p1, n.p1).pod())
            }
            (MarkShape::Text(o), MarkShape::Text(n)) => MarkShapeInterp::Text(TextMarkInterp::new(
                StringInterp::new(&o.txt, &n.txt).pod(),
                F64Interp::new(o.size, n.size).pod(),
                PointInterp::new(o.point, n.point).pod(),
            ).pod()),
            _=>MarkShapeInterp::Noop
        }
    }
}

impl HasInterp for MarkShape{
    type Interp = MarkShapeInterp;
}

impl Interp for MarkShapeInterp {
    type Value = MarkShape;

    fn interp(&self, frac: &Frac, val: &mut MarkShape) -> InterpResult{
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

    fn is_noop(&self) -> bool {
        match self{
            MarkShapeInterp::Rect(orig, other) => orig.is_noop() && other.is_noop(),
            MarkShapeInterp::Line(start, end) => start.is_noop() && end.is_noop(),
            MarkShapeInterp::Text(text) => text.is_noop(),
            MarkShapeInterp::Noop=>true
        }
    }

    fn select_anim(self, idx: usize) -> Self {
        match self{
            MarkShapeInterp::Rect(orig, other) => MarkShapeInterp::Rect(orig.select_anim(idx), other.select_anim(idx)),
            MarkShapeInterp::Line(start, end) => MarkShapeInterp::Line(start.select_anim(idx), end.select_anim(idx)),
            MarkShapeInterp::Text(text) => MarkShapeInterp::Text(text.select_anim(idx)),
            other=>other
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other){
            (MarkShapeInterp::Rect(orig1, other1), MarkShapeInterp::Rect(orig2, other2))=>MarkShapeInterp::Rect(orig1.merge(orig2), other1.merge(other2)),
            (MarkShapeInterp::Line(s1, e1), MarkShapeInterp::Line(s2, e2))=>MarkShapeInterp::Line(s1.merge(s2), e1.merge(e2)),
            (MarkShapeInterp::Text(t1), MarkShapeInterp::Text(t2))=>MarkShapeInterp::Text(t1.merge(t2)),
            (_, other)=>other
        }
    }
}

#[derive(Debug)]
enum ColorInterp {
    Rgba(Pod<F64Interp>, Pod<F64Interp>, Pod<F64Interp>, Pod<F64Interp>),
}

impl HasInterp for Color{
    type Interp = ColorInterp;
}

impl Interp for ColorInterp {
    type Value = Color;

    fn interp(&self, frac: &Frac, val: &mut Color) -> InterpResult{
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
        }
    }

    fn is_noop(&self) -> bool {
        match self{
            ColorInterp::Rgba(r, g, b, a) => r.is_noop() && g.is_noop() && b.is_noop() && a.is_noop(),
        }
    }

    fn select_anim(self, idx: usize) -> Self {
        match self{
            ColorInterp::Rgba(r, g, b, a) => ColorInterp::Rgba(r.select_anim(idx), g.select_anim(idx), b.select_anim(idx), a.select_anim(idx)),
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other){
            (ColorInterp::Rgba(r, g, b, a), ColorInterp::Rgba(r1, g1, b1, a1))=>ColorInterp::Rgba(r.merge(r1), g.merge(g1), b.merge(b1), a.merge(a1))
        }
    }
}

impl ColorInterp {
    fn new(old: Color, new: Color) -> ColorInterp {
        let (r, g, b, a) = old.as_rgba();
        let (r2, g2, b2, a2) = new.as_rgba();

        ColorInterp::Rgba(
            F64Interp::new(r, r2).pod(),
            F64Interp::new(g, g2).pod(),
            F64Interp::new(b, b2).pod(),
            F64Interp::new(a, a2).pod(),
        )
    }
}

#[derive(Debug)]
struct MarkInterp {
    shape: Pod<MarkShapeInterp>,
    color: Pod<ColorInterp>
}

impl MarkInterp {
    pub fn new(old: Mark, new: Mark) -> Pod<Self> {
        if old.same(&new){
            Pod::Noop
        }else {
            MarkInterp {
                shape: MarkShapeInterp::new(old.shape, new.shape).pod(),
                color: ColorInterp::new(old.color, new.color).pod()
            }.pod()
        }
    }
}

impl HasInterp for Mark{
    type Interp = MarkInterp;
}

impl Interp for MarkInterp {
    type Value = Mark;
    fn interp(&self, frac: &Frac, val: &mut Mark)->InterpResult {
        self.shape.interp(frac, &mut val.shape)?;
        self.color.interp(frac, &mut val.color)?;
        OK
    }

    fn is_noop(&self) -> bool {
        self.shape.is_noop() && self.color.is_noop()
    }

    fn select_anim(self, idx: usize) -> Self {
        let MarkInterp{ shape, color } = self;
        MarkInterp{
            shape: shape.select_anim(idx),
            color: color.select_anim(idx)
        }
    }

    fn merge(self, other: Self) -> Self {
        let MarkInterp{ shape, color } = self;
        let MarkInterp{ shape: s1, color: c1 } = other;
        MarkInterp{
            shape: shape.merge(s1),
            color: color.merge(c1)
        }
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
                let arr = [0.5];
                let mut mid = l.p0;
                PointInterp::new(l.p0, l.p1).interp(&Frac::new(0, &arr) , &mut mid);
                log::info!("Line enter {:?}", &mid);
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

    pub fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        // This should be done as some interpolation of the mark before paint? Maybe
        let color = if Some(self.id) == *focus {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct StateName(pub &'static str);

impl Data for StateName{
    fn same(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Data, Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum MarkId {
    Datum(DatumId),
    AxisDomain(AxisName),
    Tick(AxisName, TickLocator),
    TickText(AxisName, TickLocator),
    StateMark(StateName, u32)
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

#[derive(Clone, Debug)]
struct VisMarks {
    // TODO: slotmap?
    marks: HashMap<MarkId, Mark>
}

impl VisMarks {
    fn find_mark(&mut self, pos: Point) -> Option<&mut Mark> {
        self.marks.values_mut()
            .filter(|mark| mark.hit(pos))
            .next()
    }

    fn paint(&self, ctx: &mut PaintCtx, focus: &Option<MarkId>) {
        for mark in self.marks.values() {
            mark.paint(ctx, focus);
        }
    }

    fn build(layout_marks: Vec<DrawableAxis>,
             state_marks: Vec<Mark>,
             data_marks: Vec<Mark> ) -> VisMarks {
        let mut marks = HashMap::new();
        for mark in state_marks.into_iter().chain(data_marks.into_iter()).chain(layout_marks.into_iter().flat_map(|x|x.marks)){
            marks.insert(mark.id, mark);
        }

        VisMarks {
            marks
        }
    }
}

#[derive(Debug)]
struct VisMarksInterp {
    marks: Pod<MapInterp<MarkInterp, MarkId>>,
}

impl VisMarksInterp {
    fn make_mark_interps(
        old: HashMap<MarkId, Mark>,
        new: &HashMap<MarkId, Mark>,
    ) -> (Vec<(MarkId, Pod<MarkInterp>)>, HashMap<MarkId, Mark>) {
        let mut matched_marks: HashMap<MarkId, (Option<Mark>, Option<Mark>)> = HashMap::new();

        for (id, mark)  in old.into_iter() {
            matched_marks.entry(id)
                    .or_insert_with(|| (Some(mark), None));
        }

        for (id, mark) in new.iter() {
            matched_marks
                .entry(*id)
                .or_insert_with(|| (None, None))
                .1 = Some(mark.clone())
        }

        matched_marks
            .into_iter()
            .flat_map(|(k, v)| match v {
                (Some(o), Some(n)) => Some(( (k, MarkInterp::new( o.clone(), n)), (k, o))),
                (None, Some(n)) => {
                    let e = n.enter();
                    Some(((k,MarkInterp::new( e.clone(), n)),  (k, e)))
                },
                (Some(o), None) => {
                    let e = o.enter(); // TODO: exit
                    Some(((k, MarkInterp::new( o.clone(), e)), (k, o)))
                }
                _ => None,
            })
            .unzip()
    }


    fn build(old: VisMarks, new: &VisMarks) -> (Pod<Self>, VisMarks) {
        let (interps, marks) = Self::make_mark_interps( old.marks, &new.marks);

        let vis_marks = VisMarks{
            marks
        };

        let marks_interp = MapInterp::new(interps.into_iter());

        let vmi = if marks_interp.is_noop() {
            Pod::Noop
        } else {
           VisMarksInterp { marks: marks_interp }.pod()
        };

        (vmi, vis_marks)
    }
}

impl HasInterp for VisMarks{
    type Interp = VisMarksInterp;
}

impl Interp for VisMarksInterp {
    type Value = VisMarks;

    fn interp(&self, frac: &Frac, val: &mut VisMarks) -> InterpResult{
        self.marks.interp(frac, &mut val.marks)
    }

    fn is_noop(&self) -> bool {
        self.marks.is_noop()
    }

    fn select_anim(self, idx: usize) -> Self {
        VisMarksInterp{
            marks: self.marks.select_anim(idx)
        }
    }

    fn merge(self, other: Self) -> Self {
        Self{
            marks: self.marks.merge(other.marks),
        }
    }
}

#[derive(Debug)]
struct Anim{
    start_nanos: u64,
    end_nanos: u64
}

impl Anim {
    pub fn new(start_nanos: u64, end_nanos: u64) -> Self {
        Anim { start_nanos, end_nanos }
    }
}

struct VisTransition {
    // matched_marks: HashMap<MarkId, MarkInterp>,
    cur_nanos: u64,
    anims: Vec<Anim>,
    interp: Pod<VisMarksInterp>,
    current: VisMarks
}

impl VisTransition {
    fn advance(&mut self, nanos: u64) -> bool {
        self.cur_nanos += nanos;
        let fracs : Vec<_> = self.anims.iter().map(|a|{
            let since_start = self.cur_nanos - a.start_nanos;
            let dur = a.end_nanos - a.start_nanos;
            let frac = since_start as f64 / dur as f64;
            frac.max(0.).min( 1.)
        }).collect();
        let frac = Frac::new(0,  &fracs );
        self.interp.interp(&frac, &mut self.current);
        self.anims.iter().all(|a| self.cur_nanos >= a.end_nanos)
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
        marks: VisMarks,
        transform: Affine,
    ) -> Self {
        VisInner {
            layout,
            state,
            marks,
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

            let marks = VisMarks::build(layout_marks, state_marks, data_marks, );

            self.inner = Some(VisInner::new(
                layout,
                state,
                marks,
                Affine::FLIP_Y * Affine::translate(Vec2::new(0., -size.height)),
            ));
        }
        self.inner.as_mut().unwrap()
    }

    fn start_transition(&mut self, size: Size, data: &V::Input) {
        if let Some(inner) = &mut self.inner {

            inner.layout = self.visual.layout(data, size);

            let mut temp = VisMarks::build(
                 self.visual.layout_marks(&inner.layout),
                self.visual.state_marks(data, &inner.layout, &inner.state),
                 self.visual.data_marks(data, &inner.layout)
            );
            std::mem::swap(&mut inner.marks, &mut temp);

            let vt = if let Some(mut old_transit) = inner.transition.take() {
                let start = old_transit.current;
                let (interp, current) = VisMarksInterp::build( start, &inner.marks);
                let interp = interp.select_anim(old_transit.anims.len());
                let interp = old_transit.interp.merge(interp);
                old_transit.anims.push( Anim::new(old_transit.cur_nanos, old_transit.cur_nanos + 10 * 250 * 1_000_000) );
                VisTransition{
                    cur_nanos: old_transit.cur_nanos,
                    anims: old_transit.anims,
                    interp,
                    current
                }
            }else{
                let start = temp;
                let (interp, current) = VisMarksInterp::build( start, &inner.marks);
                VisTransition {
                    cur_nanos: 0,
                    interp: interp.select_anim(0),
                    current,
                    anims: vec![Anim::new(0, 10 * 250 * 1_000_000)]
                }
            };

            inner.transition = if vt.interp.is_noop(){
                None
            } else {
                Some(vt)
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
                    if inner.focus != Some(mark.id) {
                        self.visual.event(
                            data,
                            &inner.layout,
                            &mut inner.state,
                            &VisEvent::MouseEnter(mark.id),
                        );
                        inner.focus = Some(mark.id);
                        ctx.request_paint();
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
            self.start_transition(ctx.size(), data);
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
                transition.iter().for_each (|t| {
                    log::info!("Stopping animations {:?} {:?}", t.cur_nanos, t.anims)
                });
                *transition = None;
            } else {
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

pub struct OffsetSource<T, Id>{
    val_to_id: HashMap<T, Id>
}

impl <T: Hash + Eq, Id> Default for OffsetSource<T, Id>{
    fn default() -> Self {
        Self{
            val_to_id: Default::default()
        }
    }
}

impl <T: Clone + Hash + Eq, Id: From<usize> + Into<usize> + Clone> OffsetSource<T, Id>{
    pub fn offset(&mut self, item: &T) ->Id{
        let next: Id = self.val_to_id.len().into();
        self.val_to_id.entry(item.clone()).or_insert(next).clone()
    }
}

pub struct BandScaleFactory<T> {
    name: AxisName,
    offset_source: OffsetSource<T, PersistentId>
}

impl<T: Clone + Ord + Hash + Display> BandScaleFactory<T> {
    pub fn new(name: AxisName) -> Self {
        BandScaleFactory { name, offset_source: Default::default() }
    }

    pub fn make_scale(
        &mut self,
        range: F64Range,
        bands_it: &mut impl Iterator<Item = T>,
        padding_ratio: f64
    )->BandScale<T>{
        BandScale::new(self.name, range, &mut self.offset_source, bands_it, padding_ratio)
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
        let bands: Vec<_> = uniq.iter().map(|band|{
            let persistent_id = offset_source.offset(&band);
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


