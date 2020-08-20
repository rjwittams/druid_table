use crate::config::{DEFAULT_COL_HEADER_HEIGHT, DEFAULT_ROW_HEADER_WIDTH};
use crate::data::RemapDetails;
use crate::{AxisMeasurementType, Remap};
use druid::{Cursor, Data, Point, Rect, Size};
use float_ord::FloatOrd;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::iter::Map;
use std::ops::{Add, RangeInclusive, Sub};
use std::rc::Rc;
use TableAxis::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Data, Ord, PartialOrd)]
pub enum TableAxis {
    Rows,    // Rows means the Y axis. A single row spans the X axis, but rows stack downwards.
    Columns, // The X axis. A column is vertical, but columns go along horizontally
}

// Acts as an enum map
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct AxisPair<T: Debug> {
    pub row: T,
    pub col: T,
}

impl<T: Copy + Default + Debug> Copy for AxisPair<T> {}

impl<T: Data + Debug + Default> Data for AxisPair<T> {
    fn same(&self, other: &Self) -> bool {
        self.row.same(&other.row) && self.col.same(&other.col)
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

    pub fn main_pixel_from_point(&self, point: &Point) -> f64 {
        match self {
            Rows => point.y,
            Columns => point.x,
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

impl VisIdx {
    // Todo work out how to support custom range
    pub fn range_inc_iter(
        from_inc: VisIdx,
        to_inc: VisIdx,
    ) -> Map<RangeInclusive<usize>, fn(usize) -> VisIdx> {
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
}

#[derive(Debug, Clone)]
enum AxisMeasureInner {
    Fixed(FixedAxisMeasure),
    Stored(Rc<RefCell<StoredAxisMeasure>>),
}

use AxisMeasureInner::*;

impl AxisMeasure {
    fn border(&self) -> f64 {
        match &self.inner {
            Fixed(f) => f.border,
            Stored(s) => s.borrow().border(),
        }
    }

    pub fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) {
        if match &mut self.inner {
            Fixed(f) => f.set_axis_properties(border, len, remap),
            Stored(s) => s.borrow_mut().set_axis_properties(border, len, remap),
        } {
            self.version += 1
        }
    }

    pub fn vis_range_from_pixels(&self, p0: f64, p1: f64) -> (VisIdx, VisIdx) {
        let start = self.vis_idx_from_pixel(p0).unwrap_or(VisIdx(0));
        let end = self
            .vis_idx_from_pixel(p1)
            .unwrap_or_else(|| self.last_vis_idx());
        (start, end)
    }

    pub fn total_pixel_length(&self) -> f64 {
        match &self.inner {
            Fixed(f) => f.total_pixel_length(),
            Stored(s) => s.borrow().total_pixel_length(),
        }
    }

    fn last_vis_idx(&self) -> VisIdx {
        let len = match &self.inner {
            Fixed(f) => f.len,
            Stored(s) => s.borrow().vis_pix_lengths.len(),
        };
        VisIdx(len - 1)
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

    pub fn set_far_pixel_for_vis(&mut self, idx: VisIdx, pixel: f64) {
        // Check if changed
        if match &mut self.inner {
            Fixed(f) => f.set_far_pixel_for_vis(idx, pixel),
            Stored(s) => s.borrow_mut().set_far_pixel_for_vis(idx, pixel),
        } {
            self.version += 1;
        }
    }

    pub(crate) fn far_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        self.first_pixel_from_vis(idx)
            .map(|p| self.pixels_length_for_vis(idx).map(|l| p + l))
            .flatten()
    }
}

trait AxisMeasureT: Debug {
    fn border(&self) -> f64;

    fn total_pixel_length(&self) -> f64;
    fn vis_idx_from_pixel(&self, pixel: f64) -> Option<VisIdx>;

    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64>;
    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64>;
    fn can_resize(&self, idx: VisIdx) -> bool;

    fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool;
    fn set_far_pixel_for_vis(&mut self, idx: VisIdx, pixel: f64) -> bool;
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

    fn can_resize(&self, _idx: VisIdx) -> bool {
        false
    }

    fn set_axis_properties(&mut self, border: f64, len: usize, _remap: &Remap) -> bool {
        let changed = border != self.border || len != self.len;

        self.border = border;
        self.len = len;
        // We don't care about remap as every item is the same... I think.
        // TODO: Maybe we should care about the length?
        changed
    }

    fn set_far_pixel_for_vis(&mut self, _idx: VisIdx, _pixel: f64) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct StoredAxisMeasure {
    remap: Remap,
    log_pix_lengths: Vec<f64>,
    vis_pix_lengths: Vec<f64>,
    first_pixels: BTreeMap<VisIdx, f64>, // TODO newtypes
    pixels_to_vis: BTreeMap<FloatOrd<f64>, VisIdx>,
    default_pixels: f64,
    border: f64,
    total_pixel_length: f64,
}

impl Debug for StoredAxisMeasure {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        let fp = &self.first_pixels;
        let pti = &self.pixels_to_vis;
        fmt.debug_struct("StoredAxisMeasure")
            .field("log_pix_lengths", &self.log_pix_lengths)
            .field("vis_pix_lengths", &self.vis_pix_lengths)
            .field("default_pixels", &self.default_pixels)
            .field("border", &self.border)
            .field("total_pixel_length", &self.total_pixel_length)
            .field(
                "first_pixels",
                debug_fn!(|f| f.debug_map().entries(fp.iter()).finish()),
            )
            .field(
                "pixels_to_index",
                debug_fn!(|f| f
                    .debug_map()
                    .entries(pti.iter().map(|(k, v)| (k.0, v)))
                    .finish()),
            )
            .finish()
    }
}

impl StoredAxisMeasure {
    pub fn new(default_pixels: f64) -> Self {
        StoredAxisMeasure {
            remap: Remap::new(),
            log_pix_lengths: Default::default(),
            vis_pix_lengths: Default::default(),
            first_pixels: Default::default(),
            pixels_to_vis: Default::default(),
            default_pixels,
            border: 0.,
            total_pixel_length: 0.,
        }
    }

    fn build_maps(&mut self) {
        let mut cur = 0.;
        self.vis_pix_lengths.clear();
        if self.remap.is_pristine() {
            self.vis_pix_lengths.extend_from_slice(&self.log_pix_lengths)
        }else{
            for vis_idx in VisIdx::range_inc_iter(VisIdx(0), self.remap.max_vis_idx(self.log_pix_lengths.len())){
                if let Some(log_idx) = self.remap.get_log_idx(vis_idx) {
                    self.vis_pix_lengths.push(self.log_pix_lengths[log_idx.0]);
                }
            }
        }

        self.first_pixels.clear();
        self.pixels_to_vis.clear();
        for (idx, pixels) in self.vis_pix_lengths.iter().enumerate() {
            self.first_pixels.insert(VisIdx(idx), cur);
            self.pixels_to_vis.insert(FloatOrd(cur), VisIdx(idx));
            cur += pixels + self.border;
        }
        self.total_pixel_length = cur;
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
        self.pixels_to_vis
            .range(..=FloatOrd(pixel))
            .next_back()
            .map(|(_, v)| *v)
    }

    fn first_pixel_from_vis(&self, idx: VisIdx) -> Option<f64> {
        self.first_pixels.get(&idx).copied()
    }

    fn pixels_length_for_vis(&self, idx: VisIdx) -> Option<f64> {
        self.vis_pix_lengths.get(idx.0).copied()
    }

    fn can_resize(&self, _idx: VisIdx) -> bool {
        true
    }

    fn set_axis_properties(&mut self, border: f64, len: usize, remap: &Remap) -> bool {
        self.border = border;
        self.remap = remap.clone(); // Todo: pass by ref where needed? Or make the measure own it

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
        self.build_maps();

        true
    }

    fn set_far_pixel_for_vis(&mut self, vis_idx: VisIdx, pixel: f64) -> bool {
        let length = f64::max(0., pixel - *self.first_pixels.get(&vis_idx).unwrap_or(&0.));
        // Todo Option
        if let Some(log_idx) = self.remap.get_log_idx(vis_idx) {
            if let Some(place) = self.log_pix_lengths.get_mut(log_idx.0) {
                if *place != length {
                    *place = length;
                    self.build_maps(); // TODO : modify efficiently instead of rebuilding
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(not)]
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
        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(12), 34.), 99.);
    }

    fn test_equal_sized<AX: AxisMeasureT + Debug>(ax: &mut AX) {
        ax.set_axis_properties(1.0, 4, &Remap::Pristine);
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
            ax.vis_range_from_pixels(105.0, 295.0),
            (VisIdx(1), VisIdx(2))
        );
        assert_eq!(
            ax.vis_range_from_pixels(100.0, 300.0),
            (VisIdx(1), VisIdx(3))
        );
        let lengths = (1usize..=3)
            .into_iter()
            .map(|i| FloatOrd(ax.pixels_length_for_vis(VisIdx(i)).unwrap()))
            .collect::<HashSet<FloatOrd<f64>>>();

        assert_eq!(lengths.len(), 1);
        assert_eq!(lengths.iter().next().unwrap().0, 99.0)
    }

    #[test]
    fn stored_axis() {
        let mut ax = StoredAxisMeasure::new(99.);
        test_equal_sized(&mut ax);

        assert_eq!(ax.set_pixel_length_for_vis(VisIdx(2), 49.), 49.);
        assert_eq!(ax.set_far_pixel_for_vis(VisIdx(1), 109.), 9.);
        assert_eq!(ax.total_pixel_length(), 260.0)
    }
}
