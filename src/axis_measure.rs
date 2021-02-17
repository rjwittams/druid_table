use crate::config::{DEFAULT_COL_HEADER_HEIGHT, DEFAULT_ROW_HEADER_WIDTH};
use crate::interp::{HasInterp, Interp, InterpCoverage, InterpNode, InterpResult};
use crate::selection::CellRect;
use crate::table::PixelRange;
use crate::{AxisMeasurementType, Remap};
use druid::{Cursor, Data, Point, Rect, Size};
use druid_widget_nursery::animation::{AnimationCtx, AnimationId};
use float_ord::FloatOrd;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::ops::{Add, Sub};
use std::rc::Rc;
use AxisMeasureInner::*;
use TableAxis::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Data, Ord, PartialOrd)]
pub enum TableAxis {
    Rows,    // Rows means the Y axis. A single row spans the X axis, but rows stack downwards.
    Columns, // The X axis. A column is vertical, but columns go along horizontally
}

// Acts as an enum map
#[derive(Eq, PartialEq, Debug, Clone, Hash, Default)]
pub struct AxisPair<T> {
    pub row: T,
    pub col: T,
}

impl<T: Copy + Default> Copy for AxisPair<T> {}

impl<T: Data> Data for AxisPair<T> {
    fn same(&self, other: &Self) -> bool {
        self.row.same(&other.row) && self.col.same(&other.col)
    }
}

impl AxisPair<AxisMeasure> {
    pub(crate) fn cell_rect_from_pixels(&self, draw_rect: Rect) -> CellRect {
        CellRect::new(
            self.row.vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            self.col.vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        )
    }

    pub(crate) fn pixel_rect_for_cell(&self, vis: AxisPair<VisIdx>) -> Option<Rect> {
        let origin = self
            .zip_with(&vis, |m, vis| m.first_pixel_from_vis(*vis))
            .opt()
            .as_ref()
            .map(AxisPair::point);

        let size = self
            .zip_with(&vis, |m, vis| m.pixels_length_for_vis(*vis))
            .opt()
            .as_ref()
            .map(AxisPair::size);

        origin.zip(size).map(|(o, s)| Rect::from_origin_size(o, s))
    }

    pub(crate) fn measured_size(&self) -> Size {
        self.map(|m| m.total_pixel_length()).size()
    }
}

impl<T: HasInterp + Default> HasInterp for AxisPair<T> {
    type Interp = AxisPairInterp<T>;
}

#[derive(Default)]
pub struct AxisPairInterp<T: HasInterp> {
    pub row: InterpNode<T>,
    pub col: InterpNode<T>,
}

impl<T: HasInterp> Debug for AxisPairInterp<T>
where
    T::Interp: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // TODO: debug
        f.debug_struct("AxisPairInterp").finish()
    }
}

impl<T: HasInterp + Default> Interp for AxisPairInterp<T> {
    type Value = AxisPair<T>;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.row.interp(ctx, &mut val.row)?;
        self.col.interp(ctx, &mut val.col)
    }

    fn coverage(&self) -> InterpCoverage {
        self.col.coverage() + self.row.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            row: self.row.select_anim(idx),
            col: self.col.select_anim(idx),
        })
    }

    fn merge(self, other: Self) -> Self {
        Self {
            row: self.row.merge(other.row),
            col: self.col.merge(other.col),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self {
            row: start.row.tween(end.row),
            col: start.col.tween(end.col),
        }
    }
}

impl TableAxis {
    pub fn cross_axis(&self) -> TableAxis {
        match self {
            Rows => Columns,
            Columns => Rows,
        }
    }

    pub fn length_from_size(&self, size: &Size) -> f64 {
        match self {
            Rows => size.height,
            Columns => size.width,
        }
    }

    pub fn pixels_from_point(&self, point: &Point) -> (f64, f64) {
        match self {
            Rows => (point.y, point.x),
            Columns => (point.x, point.y)
        }
    }

    pub fn pixels_from_rect(&self, rect: &Rect) -> (f64, f64) {
        match self {
            Rows => (rect.y0, rect.y1),
            Columns => (rect.x0, rect.x1),
        }
    }

    pub fn default_header_cross(&self) -> f64 {
        match self {
            Rows => DEFAULT_ROW_HEADER_WIDTH,
            Columns => DEFAULT_COL_HEADER_HEIGHT,
        }
    }

    pub fn coords(&self, main: f64, cross: f64) -> (f64, f64) {
        match self {
            Rows => (cross, main),
            Columns => (main, cross),
        }
    }

    pub fn size(&self, main: f64, cross: f64) -> Size {
        self.coords(main, cross).into()
    }

    pub fn cell_origin(&self, main: f64, cross: f64) -> Point {
        self.coords(main, cross).into()
    }

    pub fn resize_cursor(&self) -> &'static Cursor {
        match self {
            Rows => &Cursor::ResizeUpDown,
            Columns => &Cursor::ResizeLeftRight,
        }
    }
}

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Data, Default, Hash)]
pub struct VisIdx(pub usize);

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Data, Default)]
pub struct VisOffset(pub isize);

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Data, Default, Hash)]
pub struct LogIdx(pub usize);

impl From<usize> for LogIdx {
    fn from(a: usize) -> Self {
        Self(a)
    }
}

impl From<LogIdx> for usize {
    fn from(a: LogIdx) -> Self {
        a.0
    }
}

impl VisIdx {
    // Todo work out how to support custom range
    pub fn range_inc_iter(from_inc: VisIdx, to_inc: VisIdx) -> impl Iterator<Item = VisIdx> {
        ((from_inc.0)..=(to_inc.0)).map(VisIdx)
    }

    pub(crate) fn ascending(a: VisIdx, b: VisIdx) -> (VisIdx, VisIdx) {
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    }
}

impl Add<VisOffset> for VisIdx {
    type Output = Self;

    fn add(self, rhs: VisOffset) -> Self::Output {
        // TODO this is dodgy
        VisIdx(((self.0 as isize) + rhs.0).max(0) as usize)
    }
}

impl Sub<VisOffset> for VisIdx {
    type Output = Self;

    fn sub(self, rhs: VisOffset) -> Self::Output {
        VisIdx(((self.0 as isize) - rhs.0).max(0) as usize)
    }
}

impl Sub<VisIdx> for VisIdx {
    type Output = VisOffset;

    fn sub(self, rhs: VisIdx) -> Self::Output {
        VisOffset((self.0 as isize) - (rhs.0 as isize))
    }
}

#[derive(Debug, Clone)]
pub struct AxisMeasure {
    inner: AxisMeasureInner,
    version: u64,
}

impl Data for AxisMeasure {
    fn same(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

#[derive(Debug, Clone)]
enum AxisMeasureInner {
    Fixed(FixedAxisMeasure),
    Stored(Rc<RefCell<StoredAxisMeasure>>),
}

impl AxisMeasure {
    pub fn new(amt: AxisMeasurementType, pixels_per_unit: f64) -> AxisMeasure {
        AxisMeasure {
            inner: match amt {
                AxisMeasurementType::Uniform => {
                    AxisMeasureInner::Fixed(FixedAxisMeasure::new(pixels_per_unit))
                }
                AxisMeasurementType::Individual => AxisMeasureInner::Stored(Rc::new(RefCell::new(
                    StoredAxisMeasure::new(pixels_per_unit),
                ))),
            },
            version: 0,
        }
    }

    pub fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool {
        if match &mut self.inner {
            Fixed(f) => f.set_axis_properties(border, len, remap),
            Stored(s) => s.borrow_mut().set_axis_properties(border, len, remap),
        } {
            self.version += 1;
            true
        } else {
            false
        }
    }

    pub fn set_far_pixel_for_vis(&mut self, idx: VisIdx, pixel: f64, remap: &Remap) -> bool {
        // Check if changed
        if match &mut self.inner {
            Fixed(f) => f.set_far_pixel_for_vis(idx, pixel, remap),
            Stored(s) => s.borrow_mut().set_far_pixel_for_vis(idx, pixel, remap),
        } {
            self.version += 1;
            true
        } else {
            false
        }
    }

    fn border(&self) -> f64 {
        match &self.inner {
            Fixed(f) => f.border,
            Stored(s) => s.borrow().border(),
        }
    }

    pub fn total_pixel_length(&self) -> f64 {
        match &self.inner {
            Fixed(f) => f.total_pixel_length(),
            Stored(s) => s.borrow().total_pixel_length(),
        }
    }

    fn last_vis_idx(&self) -> VisIdx {
        match &self.inner {
            Fixed(f) => VisIdx(f.len.saturating_sub(1)),
            Stored(s) => {
                let s = s.borrow();
                VisIdx(s.first_pixels.len().saturating_sub(1))
                //s.remap.max_vis_idx(s.log_pix_lengths.len())
            }
        }
    }

    pub(crate) fn vis_range_from_pixels(&self, p0: f64, p1: f64) -> (VisIdx, VisIdx) {
        let start = self.vis_idx_from_pixel(p0).unwrap_or(VisIdx(0));
        let end = self
            .vis_idx_from_pixel(p1)
            .unwrap_or_else(|| self.last_vis_idx());
        (start, end)
    }

    pub(crate) fn pixel_near_border(&self, pixel: f64) -> Option<VisIdx> {
        let idx = self.vis_idx_from_pixel(pixel)?;
        let idx_border_middle = self.first_pixel_from_vis(idx).unwrap_or(0.) - self.border() / 2.;
        let next_border_middle = self
            .first_pixel_from_vis(idx + VisOffset(1))
            .unwrap_or_else(|| self.total_pixel_length())
            - self.border() / 2.;
        if f64::abs(pixel - idx_border_middle) < MOUSE_MOVE_EPSILON {
            Some(idx)
        } else if f64::abs(pixel - next_border_middle) < MOUSE_MOVE_EPSILON {
            Some(idx + VisOffset(1))
        } else {
            None
        }
    }

    pub fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx> {
        match &self.inner {
            Fixed(f) => f.vis_idx_from_pixel(pixel),
            Stored(s) => s.borrow().vis_idx_from_pixel(pixel),
        }
    }

    pub fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        match &self.inner {
            Fixed(f) => f.first_pixel_from_vis(idx),
            Stored(s) => s.borrow().first_pixel_from_vis(idx),
        }
    }

    pub fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        match &self.inner {
            Fixed(f) => f.pixels_length_for_vis(idx),
            Stored(s) => s.borrow().pixels_length_for_vis(idx),
        }
    }

    pub fn can_resize(&self, idx: VisIdx) -> bool {
        match &self.inner {
            Fixed(f) => f.can_resize(idx),
            Stored(s) => s.borrow().can_resize(idx),
        }
    }


}

pub trait PixelLengths{
    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64>;
    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64>;

    fn far_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        self.first_pixel_from_vis(idx)
            .and_then(|p| self.pixels_length_for_vis(idx).map(|l| p + l))
    }

    fn pix_range_from_vis(&self, idx: VisIdx) -> Option<PixelRange> {
        Some(PixelRange::new(
            self.first_pixel_from_vis(idx)?,
            self.far_pixel_from_vis(idx)?,
        ))
    }

    fn pix_range_from_vis_span(&self, idx: VisIdx, span: VisOffset) -> Option<PixelRange> {
        if span.0 >= 0 {
            Some(PixelRange::new(
                self.first_pixel_from_vis(idx)?,
                self.far_pixel_from_vis(idx + span)?,
            ))
        }else{
            None
        }
    }
}

trait AxisMeasureT: PixelLengths {
    fn border(&self) -> f64;

    fn total_pixel_length(&self) -> f64;
    fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx>;

    fn can_resize(&self, idx: VisIdx) -> bool;

    fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool;
    fn set_far_pixel_for_vis(&mut self, idx: VisIdx, pixel: f64, remap: &Remap) -> bool;


}

impl PixelLengths for AxisMeasure{
    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        AxisMeasure::first_pixel_from_vis(self, idx)
    }

    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        AxisMeasure::pixels_length_for_vis(self, idx)
    }
}

impl AxisMeasureT for AxisMeasure {
    fn border(&self) -> f64 {
        AxisMeasure::border(self)
    }

    fn total_pixel_length(&self) -> f64 {
        AxisMeasure::total_pixel_length(self)
    }

    fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx> {
        AxisMeasure::vis_idx_from_pixel(self, pixel)
    }

    fn can_resize(&self, idx: VisIdx) -> bool {
        AxisMeasure::can_resize(self, idx)
    }

    fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool {
        AxisMeasure::set_axis_properties(self, border, len, remap)
    }

    fn set_far_pixel_for_vis(&mut self, idx: VisIdx, pixel: f64, remap: &Remap) -> bool {
        AxisMeasure::set_far_pixel_for_vis(self, idx, pixel, remap)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedAxisMeasure {
    pixels_per_unit: f64,
    border: f64,
    len: usize,
}

impl FixedAxisMeasure {
    pub fn new(pixels_per_unit: f64) -> Self {
        FixedAxisMeasure {
            pixels_per_unit,
            border: 0.,
            len: 0,
        }
    }

    fn full_pixels_per_unit(&self) -> f64 {
        self.pixels_per_unit + self.border
    }
}

const MOUSE_MOVE_EPSILON: f64 = 3.;

impl PixelLengths for FixedAxisMeasure{
    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        if idx.0 < self.len {
            Some((idx.0 as f64) * self.full_pixels_per_unit())
        } else {
            None
        }
    }

    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        if idx.0 < self.len {
            Some(self.pixels_per_unit)
        } else {
            None
        }
    }
}

impl AxisMeasureT for FixedAxisMeasure {
    fn border(&self) -> f64 {
        self.border
    }

    fn total_pixel_length(&self) -> f64 {
        self.full_pixels_per_unit() * (self.len as f64)
    }

    fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx> {
        let index = (pixel / self.full_pixels_per_unit()).floor() as usize;
        if index < self.len {
            Some(VisIdx(index))
        } else {
            None
        }
    }

    fn can_resize(&self, _idx: VisIdx) -> bool {
        false
    }

    fn set_axis_properties(&mut self, border: f64, len: usize, _remap: &Remap) -> bool {
        let changed = border != self.border || len != self.len;

        self.border = border;
        self.len = len;
        // We don't care a.bout remap as every item is the same... I think.
        // TODO: Maybe we should care about the length?
        changed
    }

    fn set_far_pixel_for_vis(&mut self, _idx: VisIdx, _pixel: f64, _remap: &Remap) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct StoredAxisMeasure {
    log_pix_lengths: Vec<f64>,
    first_pixels: Vec<FloatOrd<f64>>, // Each VisIdx first pixel
    default_pixels: f64,
    border: f64,
    total_pixel_length: f64,
}

impl Debug for StoredAxisMeasure {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        let fp = &self.first_pixels;
        fmt.debug_struct("StoredAxisMeasure")
            .field("log_pix_lengths", &self.log_pix_lengths)
            .field("default_pixels", &self.default_pixels)
            .field("border", &self.border)
            .field("total_pixel_length", &self.total_pixel_length)
            .field(
                "first_pixels",
                debug_fn!(|f| f.debug_list().entries(fp.iter().map(|f| f.0)).finish()),
            )
            .finish()
    }
}

impl StoredAxisMeasure {
    pub fn new(default_pixels: f64) -> Self {
        StoredAxisMeasure {
            log_pix_lengths: Default::default(),
            first_pixels: Default::default(),
            default_pixels,
            border: 0.,
            total_pixel_length: 0.,
        }
    }

    fn refresh(&mut self, remap: &Remap) {
        let mut pixels_so_far = 0.;
        self.first_pixels.clear();
        for vis_idx in
            VisIdx::range_inc_iter(VisIdx(0), remap.max_vis_idx(self.log_pix_lengths.len()))
        {
            if let Some(log_idx) = remap.get_log_idx(vis_idx) {
                self.first_pixels.push(FloatOrd(pixels_so_far));
                pixels_so_far += self.log_pix_lengths[log_idx.0] + self.border;
            }
        }

        self.total_pixel_length = pixels_so_far;
    }
}

impl PixelLengths for StoredAxisMeasure{
    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        self.first_pixels.get(idx.0).map(|f| f.0)
    }

    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        let start = self.first_pixel_from_vis(idx)?;
        let end = if idx.0 == self.first_pixels.len() - 1 {
            self.total_pixel_length
        } else {
            self.first_pixel_from_vis(idx + VisOffset(1))?
        };
        Some(end - self.border - start)
    }
}

impl AxisMeasureT for StoredAxisMeasure {
    fn border(&self) -> f64 {
        self.border
    }

    fn total_pixel_length(&self) -> f64 {
        self.total_pixel_length
    }

    fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx> {
        match self.first_pixels.binary_search(&FloatOrd(pixel)) {
            Result::Ok(idx) => Some(VisIdx(idx)),
            Result::Err(idx) => Some(VisIdx(idx.saturating_sub(1))),
        }
    }

    fn can_resize(&self, _idx: VisIdx) -> bool {
        true
    }

    fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool {
        self.border = border;
        let old_len = self.log_pix_lengths.len();

        match old_len.cmp(&len) {
            Ordering::Greater => self.log_pix_lengths.truncate(len),
            Ordering::Less => {
                let extra = vec![self.default_pixels; len - old_len];
                self.log_pix_lengths.extend_from_slice(&extra[..]);
                assert_eq!(self.log_pix_lengths.len(), len);
            }
            _ => (),
        }
        self.refresh(&remap);

        true
    }

    fn set_far_pixel_for_vis(&mut self, vis_idx: VisIdx, pixel: f64, remap: &Remap) -> bool {
        let length = f64::max(
            0.,
            pixel - self.first_pixels.get(vis_idx.0).map(|f| f.0).unwrap_or(0.),
        );

        if let Some(log_idx) = remap.get_log_idx(vis_idx) {
            if let Some(place) = self.log_pix_lengths.get_mut(log_idx.0) {
                if *place != length {
                    *place = length;
                    self.refresh(remap);
                    return true;
                }
            }
        }
        false
    }
}

pub(crate) struct OverriddenPixelLengths<'a,'b, 'c, PL> {
    und: &'a PL,
    remap: &'b Remap,
    overrides: &'c HashMap<LogIdx, PixelRange>
}

impl<'a, 'b, 'c, PL> OverriddenPixelLengths<'a, 'b, 'c, PL> {
    pub fn new(und: &'a PL, remap: &'b Remap, overrides: &'c HashMap<LogIdx, PixelRange>) -> Self {
        OverriddenPixelLengths { und, remap, overrides }
    }
}


impl <'a,'b,'c, PL: PixelLengths>  PixelLengths for OverriddenPixelLengths<'a,'b, 'c, PL>{
    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        self.remap.get_log_idx(idx)
            .and_then(|log| self.overrides.get(&log).map(|ov|ov.p_0))
            .or_else(|| self.und.first_pixel_from_vis(idx))
    }

    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        self.remap.get_log_idx(idx)
            .and_then(|log| self.overrides.get(&log).map(|ov|ov.extent()))
            .or_else(|| self.und.pixels_length_for_vis(idx))
    }
}

#[cfg(test)]
mod test {
    use crate::axis_measure::{AxisMeasureT, VisIdx};
    use crate::{FixedAxisMeasure, Remap, StoredAxisMeasure};
    use float_ord::FloatOrd;
    use std::collections::HashSet;
    use std::fmt::Debug;

    #[test]
    fn fixed_axis() {
        let mut ax = FixedAxisMeasure::new(99.0);

        test_equal_sized(&mut ax);
        let remap = Remap::Pristine(10);
        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(12), 34., &remap), false);
    }

    fn test_equal_sized<AX: AxisMeasureT + Debug>(ax: &mut AX) {
        ax.set_axis_properties(1.0, 4, &Remap::Pristine(4));
        println!("Axis:{:#?}", ax);
        assert_eq!(ax.total_pixel_length(), 400.);
        assert_eq!(ax.vis_idx_from_pixel(350.0), Some(VisIdx(3)));
        assert_eq!(ax.first_pixel_from_vis(VisIdx(0)), Some(0.));
        assert_eq!(ax.vis_idx_from_pixel(0.0), Some(VisIdx(0)));
        assert_eq!(ax.vis_idx_from_pixel(100.0), Some(VisIdx(1)));
        assert_eq!(ax.vis_idx_from_pixel(1.0), Some(VisIdx(0)));
        assert_eq!(ax.first_pixel_from_vis(VisIdx(1)), Some(100.0));

        assert_eq!(
            (199..=201)
                .into_iter()
                .map(|n| ax.vis_idx_from_pixel(n as f64).unwrap())
                .collect::<Vec<VisIdx>>(),
            vec![VisIdx(1), VisIdx(2), VisIdx(2)]
        );

        assert_eq!(
            (ax.vis_idx_from_pixel(105.0), ax.vis_idx_from_pixel(295.0)),
            (Some(VisIdx(1)), Some(VisIdx(2)))
        );
        assert_eq!(
            (ax.vis_idx_from_pixel(100.0), ax.vis_idx_from_pixel(300.0)),
            (Some(VisIdx(1)), Some(VisIdx(3)))
        );
        let lengths = (1usize..=3)
            .into_iter()
            .map(|i| FloatOrd(ax.pixels_length_for_vis(VisIdx(i)).unwrap()))
            .collect::<HashSet<FloatOrd<f64>>>();

        assert_eq!(lengths.len(), 1);
        assert_eq!(lengths.iter().next().unwrap().0, 99.0)
    }

    #[test]
    fn stored_axis_equal() {
        let remap = Remap::Pristine(4);
        let mut ax = StoredAxisMeasure::new(100.);
        test_equal_sized(&mut ax);

        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(2), 160., &remap), true);
        assert_eq!(ax.total_pixel_length(), 160.0);
        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(1), 110., &remap), true);
        assert_eq!(ax.total_pixel_length(), 170.0)
    }

    #[test]
    fn stored_axis_() {
        let remap = Remap::Pristine(4);
        let mut ax = StoredAxisMeasure::new(99.);
        ax.set_axis_properties(1.0, 2, &remap);

        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(1), 159., &remap), true);
        assert_eq!(ax.total_pixel_length(), 160.0);
        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(0), 109., &remap), true);
        assert_eq!(ax.total_pixel_length(), 170.0)
    }
}
