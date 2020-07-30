
use std::collections::BTreeMap;
use float_ord::FloatOrd;
use druid::{Selector, EventCtx};

#[derive(Debug, Clone, Copy)]
pub enum TableAxis{
    //Rows,
    Columns
}

#[derive(Debug, Clone, Copy)]
pub enum AxisMeasureAdjustment{
    LengthChanged(TableAxis, usize, f64)
}

pub const ADJUST_AXIS_MEASURE: Selector<AxisMeasureAdjustment> =
    Selector::new("druid-builtin.table.adjust-measure");

pub type AxisMeasureAdjustmentHandler = dyn Fn(&mut EventCtx, &AxisMeasureAdjustment);

pub trait AxisMeasure: Clone {
    fn border(&self)->f64;
    fn set_axis_properties(&mut self, border: f64, len: usize);
    fn total_pixel_length(&self) -> f64;
    fn index_from_pixel(&self, pixel: f64) -> Option<usize>;
    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize);
    fn first_pixel_from_index(&self, idx: usize) -> Option<f64>;
    fn pixels_length_for_index(&self, idx: usize) -> Option<f64>;
    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64;
    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64;
    fn can_resize(&self, idx: usize)->bool;

    fn pixel_near_border(&self, pixel: f64) -> Option<usize> {
        let idx = self.index_from_pixel(pixel)?;
        let idx_border_middle = self.first_pixel_from_index(idx).unwrap_or(0.) - self.border() / 2.;
        let next_border_middle = self.first_pixel_from_index(idx + 1).unwrap_or_else(||self.total_pixel_length()) - self.border() / 2.;
        if f64::abs(pixel - idx_border_middle ) < MOUSE_MOVE_EPSILON {
            Some(idx)
        }else if f64::abs(pixel - next_border_middle ) < MOUSE_MOVE_EPSILON {
            Some(idx + 1)
        }else{
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedSizeAxis {
    pixels_per_unit: f64,
    border: f64,
    len: usize,
}

impl FixedSizeAxis {
    pub fn new(pixels_per_unit: f64) -> Self {
        FixedSizeAxis {
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

impl AxisMeasure for FixedSizeAxis {
    fn border(&self)->f64 {
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
        }else{
            None
        }
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        if idx < self.len {
            Some(self.pixels_per_unit)
        }else{
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
pub struct StoredAxisMeasure{
    pixel_lengths: Vec<f64>,
    first_pixels : BTreeMap<usize, f64>,
    pixels_to_index: BTreeMap<FloatOrd<f64>, usize>,
    default_pixels: f64,
    border: f64,
    total_pixel_length: f64
}

impl StoredAxisMeasure{
    pub fn new(default_pixels: f64) -> Self {
        StoredAxisMeasure {
            pixel_lengths: Default::default(),
            first_pixels: Default::default(),
            pixels_to_index: Default::default(),
            default_pixels,
            border: 0.,
            total_pixel_length: 0.
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


impl AxisMeasure for StoredAxisMeasure{
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
        self.pixels_to_index.range( .. FloatOrd(pixel) ).next_back().map(|(_, v)| *v)
    }

    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize) {
        let mut iter = self.pixels_to_index.range( FloatOrd(p0)..FloatOrd(p1) ).map(|(_,v)|*v);
        let (start, end) = (iter.next(), iter.next_back());

        let start = start.map(|i| if i == 0 {0 } else { i - 1} ).unwrap_or(0);
        let end = end.unwrap_or(self.pixel_lengths.len() - 1);
        (start, end)
    }

    fn first_pixel_from_index(&self, idx: usize) -> Option<f64> {
        self.first_pixels.get(&idx).copied()
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        self.pixel_lengths.get(idx).copied()
    }

    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64 {
        let length = f64::max(0.,  pixel - *self.first_pixels.get(&idx).unwrap_or(&0.));
        self.set_pixel_length_for_idx(idx, length)
    }

    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64 {
        self.pixel_lengths[idx] = length;
        self.build_maps(); // TODO : modify efficiently instead of rebuilding
        length
    }

    fn can_resize(&self, _idx: usize) -> bool {
        true
    }
}