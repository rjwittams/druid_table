use druid::{EventCtx, Point, Rect, Selector, Size, Cursor};
use float_ord::FloatOrd;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Debug, Formatter};

#[derive(Debug, Clone, Copy)]
pub enum TableAxis {
    Rows,
    Columns,
}

use crate::config::{DEFAULT_COL_HEADER_HEIGHT, DEFAULT_ROW_HEADER_WIDTH};
use TableAxis::*;

impl TableAxis {
    pub fn cross_axis(&self) -> TableAxis {
        match self {
            Rows => Columns,
            Columns => Rows,
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

    pub fn resize_cursor(&self)->&'static Cursor{
        match self{
            Rows => &Cursor::ResizeUpDown,
            Columns => &Cursor::ResizeLeftRight
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AxisMeasureAdjustment {
    LengthChanged(TableAxis, usize, f64),
}

pub const ADJUST_AXIS_MEASURE: Selector<AxisMeasureAdjustment> =
    Selector::new("druid-builtin.table.adjust-measure");

pub type AxisMeasureAdjustmentHandler = dyn Fn(&mut EventCtx, &AxisMeasureAdjustment);

pub trait AxisMeasure: Clone {
    fn border(&self) -> f64;
    fn set_axis_properties(&mut self, border: f64, len: usize);
    fn total_pixel_length(&self) -> f64;
    fn index_from_pixel(&self, pixel: f64) -> Option<usize>;
    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize);
    fn first_pixel_from_index(&self, idx: usize) -> Option<f64>;
    fn pixels_length_for_index(&self, idx: usize) -> Option<f64>;
    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64;
    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64;
    fn can_resize(&self, idx: usize) -> bool;

    fn pixel_near_border(&self, pixel: f64) -> Option<usize> {
        let idx = self.index_from_pixel(pixel)?;
        let idx_border_middle = self.first_pixel_from_index(idx).unwrap_or(0.) - self.border() / 2.;
        let next_border_middle = self
            .first_pixel_from_index(idx + 1)
            .unwrap_or_else(|| self.total_pixel_length())
            - self.border() / 2.;
        if f64::abs(pixel - idx_border_middle) < MOUSE_MOVE_EPSILON {
            Some(idx)
        } else if f64::abs(pixel - next_border_middle) < MOUSE_MOVE_EPSILON {
            Some(idx + 1)
        } else {
            None
        }
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

impl AxisMeasure for FixedAxisMeasure {
    fn border(&self) -> f64 {
        self.border
    }

    fn set_axis_properties(&mut self, border: f64, len: usize) {
        self.border = border;
        self.len = len;
    }

    fn total_pixel_length(&self) -> f64 {
        self.full_pixels_per_unit() * (self.len as f64)
    }

    fn index_from_pixel(&self, pixel: f64) -> Option<usize> {
        let index = (pixel / self.full_pixels_per_unit()).floor() as usize;
        if index < self.len {
            Some(index)
        } else {
            None
        }
    }

    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize) {
        let start = self.index_from_pixel(p0);
        let end = self.index_from_pixel(p1);

        let start = start.unwrap_or(0);
        let end = end.unwrap_or(self.len - 1);
        (start, end)
    }

    fn first_pixel_from_index(&self, idx: usize) -> Option<f64> {
        if idx < self.len {
            Some((idx as f64) * self.full_pixels_per_unit())
        } else {
            None
        }
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        if idx < self.len {
            Some(self.pixels_per_unit)
        } else {
            None
        }
    }

    fn set_far_pixel_for_idx(&mut self, _idx: usize, _pixel: f64) -> f64 {
        self.pixels_per_unit
    }

    fn set_pixel_length_for_idx(&mut self, _idx: usize, _length: f64) -> f64 {
        self.pixels_per_unit
    }

    fn can_resize(&self, _idx: usize) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct StoredAxisMeasure {
    pixel_lengths: Vec<f64>,
    first_pixels: BTreeMap<usize, f64>, // TODO newtypes
    pixels_to_index: BTreeMap<FloatOrd<f64>, usize>,
    default_pixels: f64,
    border: f64,
    total_pixel_length: f64,
}

struct DebugFn<'a, F: Fn(&mut Formatter) -> fmt::Result>(&'a F);

impl<'a, F: Fn(&mut Formatter) -> fmt::Result> Debug for DebugFn<'a, F> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let func = self.0;
        (func)(f)
    }
}

macro_rules! debug_fn {
    ($content: expr) => {
        &DebugFn(&$content)
    };
}

impl Debug for StoredAxisMeasure {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        let fp = &self.first_pixels;
        let pti = &self.pixels_to_index;
        fmt.debug_struct("StoredAxisMeasure")
            .field("pixel_lengths", &self.pixel_lengths)
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
            pixel_lengths: Default::default(),
            first_pixels: Default::default(),
            pixels_to_index: Default::default(),
            default_pixels,
            border: 0.,
            total_pixel_length: 0.,
        }
    }

    fn build_maps(&mut self) {
        let mut cur = 0.;
        self.first_pixels.clear();
        self.pixels_to_index.clear();
        for (idx, pixels) in self.pixel_lengths.iter().enumerate() {
            self.first_pixels.insert(idx, cur);
            self.pixels_to_index.insert(FloatOrd(cur), idx);
            cur += pixels + self.border;
        }
        self.total_pixel_length = cur;
    }
}

impl AxisMeasure for StoredAxisMeasure {
    fn border(&self) -> f64 {
        self.border
    }

    fn set_axis_properties(&mut self, border: f64, len: usize) {
        self.border = border;
        self.pixel_lengths = vec![self.default_pixels; len];
        // TODO: handle resize
        self.build_maps()
    }

    fn total_pixel_length(&self) -> f64 {
        self.total_pixel_length
    }

    fn index_from_pixel(&self, pixel: f64) -> Option<usize> {
        self.pixels_to_index
            .range(..=FloatOrd(pixel))
            .next_back()
            .map(|(_, v)| *v)
    }

    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize) {
        (
            self.index_from_pixel(p0).unwrap_or(0),
            self.index_from_pixel(p1)
                .unwrap_or(self.pixel_lengths.len() - 1),
        )
    }

    fn first_pixel_from_index(&self, idx: usize) -> Option<f64> {
        self.first_pixels.get(&idx).copied()
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        self.pixel_lengths.get(idx).copied()
    }

    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64 {
        let length = f64::max(0., pixel - *self.first_pixels.get(&idx).unwrap_or(&0.));
        self.set_pixel_length_for_idx(idx, length)
    }

    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64 {
        // Todo Option
        if let Some(place) = self.pixel_lengths.get_mut(idx) {
            *place = length;
            self.build_maps(); // TODO : modify efficiently instead of rebuilding
            length
        } else {
            0.
        }
    }

    fn can_resize(&self, _idx: usize) -> bool {
        true
    }
}

#[cfg(test)]
mod test {
    use crate::{AxisMeasure, FixedAxisMeasure, StoredAxisMeasure};
    use float_ord::FloatOrd;
    use std::collections::HashSet;
    use std::fmt::Debug;

    #[test]
    fn fixed_axis() {
        let mut ax = FixedAxisMeasure::new(99.0);

        test_equal_sized(&mut ax);
        assert_eq!(ax.set_far_pixel_for_idx(12, 34.), 99.);
    }

    fn test_equal_sized<AX: AxisMeasure + Debug>(ax: &mut AX) {
        ax.set_axis_properties(1.0, 4);
        println!("Axis:{:#?}", ax);
        assert_eq!(ax.total_pixel_length(), 400.);
        assert_eq!(ax.index_from_pixel(350.0), Some(3));
        assert_eq!(ax.first_pixel_from_index(0), Some(0.));
        assert_eq!(ax.index_from_pixel(0.0), Some(0));
        assert_eq!(ax.index_from_pixel(100.0), Some(1));
        assert_eq!(ax.index_from_pixel(1.0), Some(0));
        assert_eq!(ax.first_pixel_from_index(1), Some(100.0));

        assert_eq!(
            (199..=201)
                .into_iter()
                .map(|n| ax.index_from_pixel(n as f64).unwrap())
                .collect::<Vec<usize>>(),
            vec![1, 2, 2]
        );

        assert_eq!(ax.index_range_from_pixels(105.0, 295.0), (1, 2));
        assert_eq!(ax.index_range_from_pixels(100.0, 300.0), (1, 3));
        let lengths = (1usize..=3)
            .into_iter()
            .map(|i| FloatOrd(ax.pixels_length_for_index(i).unwrap()))
            .collect::<HashSet<FloatOrd<f64>>>();

        assert_eq!(lengths.len(), 1);
        assert_eq!(lengths.iter().next().unwrap().0, 99.0)
    }

    #[test]
    fn stored_axis() {
        let mut ax = StoredAxisMeasure::new(99.);
        test_equal_sized(&mut ax);

        assert_eq!(ax.set_pixel_length_for_idx(2, 49.), 49.);
        assert_eq!(ax.set_far_pixel_for_idx(1, 109.), 9.);
        assert_eq!(ax.total_pixel_length(), 260.0)
    }
}
