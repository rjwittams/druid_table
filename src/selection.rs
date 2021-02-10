use crate::axis_measure::{AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::AxisMeasure;
use druid::kurbo::{Point, Rect, Size};
use std::fmt::Debug;
use std::iter::Map;
use std::ops::{Add, Index, IndexMut, RangeInclusive};

// Could be the address of a cell or something else we have one of for each axis

impl<T: Debug> AxisPair<T> {

    pub fn new(row: T, col: T) -> AxisPair<T> {
        AxisPair { row, col }
    }

    pub fn new_for_axis(axis: TableAxis, main: T, cross: T) -> AxisPair<T>
    where
        T: Default,
    {
        let mut ca = AxisPair::new(Default::default(), Default::default());
        ca[axis] = main;
        ca[axis.cross_axis()] = cross;
        ca
    }

    pub fn from_tuple((row, col): (T, T)) -> AxisPair<T> {
        AxisPair { row, col }
    }

    pub fn for_each(&self, f: impl Fn(TableAxis, &T)) {
        f(TableAxis::Rows, &self.row);
        f(TableAxis::Columns, &self.col);
    }

    pub fn map<O: Debug>(&self, f: impl Fn(&T) -> O) -> AxisPair<O> {
        AxisPair::new(f(&self.row), f(&self.col))
    }

    pub fn zip_with<O: Debug, U: Debug>(
        &self,
        other: &AxisPair<U>,
        f: impl Fn(&T, &U) -> O,
    ) -> AxisPair<O> {
        AxisPair::new(f(&self.row, &other.row), f(&self.col, &other.col))
    }
}

impl<T: Debug + Copy> AxisPair<Option<T>> {
    pub fn opt(&self) -> Option<AxisPair<T>> {
        match (&self.row, &self.col) {
            (Some(r), Some(c)) => Some(AxisPair::new(*r, *c)),
            _ => None,
        }
    }
}

impl AxisPair<f64> {

    // For conversion to points/ sizes
    fn unpack(&self) -> (f64, f64) {
        (self[TableAxis::Columns], self[TableAxis::Rows])
    }

    pub fn size(&self) -> Size {
        self.unpack().into()
    }

    pub fn point(&self) -> Point {
        self.unpack().into()
    }
}

// For now a rect only makes sense in VisIdx - In LogIdx any list of points is possible due to remapping
#[derive(Debug)]
pub struct CellRect {
    pub start_row: VisIdx,
    pub end_row: VisIdx,
    pub start_col: VisIdx,
    pub end_col: VisIdx,
}

impl CellRect {
    pub fn new(
        (start_row, end_row): (VisIdx, VisIdx),
        (start_col, end_col): (VisIdx, VisIdx),
    ) -> CellRect {
        CellRect {
            start_row,
            end_row,
            start_col,
            end_col,
        }
    }

    pub fn point(point: AxisPair<VisIdx>) -> CellRect {
        CellRect::new((point.row, point.row), (point.col, point.col))
    }

    pub fn rows(&self) -> impl Iterator<Item=VisIdx> {
        VisIdx::range_inc_iter(self.start_row, self.end_row) // Todo work out how to support custom range
    }

    pub fn cols(&self) -> impl Iterator<Item=VisIdx> {
        VisIdx::range_inc_iter(self.start_col, self.end_col)
    }

    pub fn cells(&self) -> impl Iterator<Item=AxisPair<VisIdx>> {
        let (start_col, end_col) = (self.start_col, self.end_col);
        VisIdx::range_inc_iter(self.start_row, self.end_row)
            .flat_map(move |row| VisIdx::range_inc_iter(start_col, end_col).map(move |col|AxisPair::new(row, col)))
    }

    fn contains_cell(&self, cell_addr: &AxisPair<VisIdx>) -> bool {
        self.contains_idx(TableAxis::Columns, cell_addr.col)
            && self.contains_idx(TableAxis::Rows, cell_addr.row)
    }

    pub (crate) fn range(&self, axis: TableAxis) -> (VisIdx, VisIdx) {
        match axis {
            TableAxis::Rows => (self.start_row, self.end_row),
            TableAxis::Columns => (self.start_col, self.end_col),
        }
    }

    fn contains_idx(&self, axis: TableAxis, idx: VisIdx) -> bool {
        let (start, end) = self.range(axis);
        start <= idx && end >= idx
    }

    pub fn to_pixel_rect(&self, measures: &AxisPair<AxisMeasure>) -> Option<Rect> {
        let col_measure = &measures[TableAxis::Columns];
        let row_measure = &measures[TableAxis::Rows];

        Some(Rect::new(
            col_measure.first_pixel_from_vis(self.start_col)?,
            row_measure.first_pixel_from_vis(self.start_row)?,
            col_measure
                .far_pixel_from_vis(self.end_col)
                .unwrap_or_else(|| col_measure.total_pixel_length()),
            row_measure
                .far_pixel_from_vis(self.end_row)
                .unwrap_or_else(|| row_measure.total_pixel_length()),
        ))
    }
}

trait AxisPairMove<O> {
    fn move_by(&self, axis: TableAxis, amount: O) -> Self;
}

impl<O, T: Add<O, Output = T> + Copy + Debug + Default> AxisPairMove<O> for AxisPair<T> {
    fn move_by(&self, axis: TableAxis, amount: O) -> AxisPair<T> {
        let mut moved = (*self).clone();
        moved[axis] = self[axis] + amount;
        moved
    }
}

impl<T: Debug> Index<TableAxis> for AxisPair<T> {
    type Output = T;

    fn index(&self, axis: TableAxis) -> &Self::Output {
        match axis {
            TableAxis::Rows => &self.row,
            TableAxis::Columns => &self.col,
        }
    }
}

impl<T: Debug> IndexMut<TableAxis> for AxisPair<T> {
    fn index_mut(&mut self, axis: TableAxis) -> &mut Self::Output {
        match axis {
            TableAxis::Rows => &mut self.row,
            TableAxis::Columns => &mut self.col,
        }
    }
}

#[derive(Data, Debug, Clone, Eq, PartialEq)]
pub struct SingleCell {
    pub vis: AxisPair<VisIdx>,
    pub log: AxisPair<LogIdx>,
}

impl SingleCell {
    pub fn new(vis: AxisPair<VisIdx>, log: AxisPair<LogIdx>) -> Self {
        SingleCell { vis, log }
    }
}

// Represents a Row or Column. Better name would be nice!
#[derive(Data, Debug, Clone, Eq, PartialEq)]
pub struct SingleSlice {
    pub axis: TableAxis,
    pub focus: SingleCell, // The cell we are focused on, that determines the slice
}

#[derive(Data, Debug, Clone)]
pub struct SliceRange {
    pub axis: TableAxis,
    pub range: CellRange,
}

impl SliceRange {
    pub fn to_cell_rect(&self, (cross_s, cross_e): (VisIdx, VisIdx)) -> CellRect {
        let main = VisIdx::ascending(
            self.range.focus.vis[self.axis],
            self.range.extent.vis[self.axis],
        );
        let cross = (cross_s + VisOffset(-1), cross_e + VisOffset(1));
        match &self.axis {
            TableAxis::Rows => CellRect::new(main, cross),
            TableAxis::Columns => CellRect::new(cross, main),
        }
    }
}

#[derive(Data, Debug, Clone, Eq, PartialEq)]
pub struct CellRange {
    pub focus: SingleCell,
    pub extent: SingleCell,
}

impl CellRange {
    pub fn new(focus: SingleCell, extent: SingleCell) -> Self {
        CellRange { focus, extent }
    }

    pub fn contains(&self, axis: TableAxis, vis_idx: VisIdx) -> bool {
        let (a, b) = VisIdx::ascending(self.focus.vis[axis], self.extent.vis[axis]);
        a <= vis_idx && vis_idx <= b
    }
}

impl SingleSlice {
    pub fn new(axis: TableAxis, focus: SingleCell) -> Self {
        SingleSlice { axis, focus }
    }

    pub fn to_cell_rect(&self, (cross_s, cross_e): (VisIdx, VisIdx)) -> CellRect {
        let main = self.focus.vis[self.axis];
        let main = (main, main);
        let cross = (cross_s + VisOffset(-1), cross_e + VisOffset(1));
        match &self.axis {
            TableAxis::Rows => CellRect::new(main, cross),
            TableAxis::Columns => CellRect::new(cross, main),
        }
    }
}

#[derive(Debug, Clone)]
pub enum IndicesSelection {
    NoSelection,
    Single(VisIdx),
    Range { focus: VisIdx, extent: VisIdx },
    //Range(from, to)
}

impl IndicesSelection {
    pub(crate) fn vis_index_selected(&self, vis_idx: VisIdx) -> bool {
        match self {
            IndicesSelection::Single(sel_vis) => *sel_vis == vis_idx,
            IndicesSelection::Range { focus, extent } => {
                let (min, max) = VisIdx::ascending(*focus, *extent);
                vis_idx >= min && vis_idx <= max
            }
            _ => false,
        }
    }
}

#[derive(Data, Debug, Clone)]
pub enum TableSelection {
    NoSelection,
    SingleCell(SingleCell),
    SingleSlice(SingleSlice),
    CellRange(CellRange),
    SliceRange(SliceRange), //  Discontiguous
}

impl Default for TableSelection {
    fn default() -> Self {
        Self::NoSelection
    }
}

pub trait CellDemap {
    fn get_log_idx(&self, axis: TableAxis, vis: &VisIdx) -> Option<LogIdx>;

    fn get_log_cell(&self, vis: &AxisPair<VisIdx>) -> Option<AxisPair<LogIdx>> {
        self.get_log_idx(TableAxis::Rows, &vis.row)
            .map(|row| {
                self.get_log_idx(TableAxis::Columns, &vis.col)
                    .map(|col| AxisPair::new(row, col))
            })
            .flatten()
    }
}

pub trait TableSelectionMod {
    fn new_selection(&self, sel: &TableSelection) -> Option<TableSelection>;
}

impl<F: Fn(&TableSelection) -> Option<TableSelection>> TableSelectionMod for F {
    fn new_selection(&self, sel: &TableSelection) -> Option<TableSelection> {
        self(sel)
    }
}

#[derive(Debug, Default)]
pub struct DrawableSelections {
    pub focus: Option<AxisPair<VisIdx>>,
    pub ranges: Vec<CellRect>,
}

impl DrawableSelections {
    pub fn new(focus: Option<AxisPair<VisIdx>>, ranges: Vec<CellRect>) -> Self {
        DrawableSelections { focus, ranges }
    }
}

impl TableSelection {
    pub fn move_focus(
        &self,
        axis: TableAxis,
        amount: VisOffset,
        cell_demap: &impl CellDemap,
    ) -> Option<TableSelection> {
        match self {
            Self::NoSelection => {
                let vis_origin = AxisPair::new(VisIdx(0), VisIdx(0));
                cell_demap
                    .get_log_cell(&vis_origin)
                    .map(|log| Self::SingleCell(SingleCell::new(vis_origin, log)))
            }
            Self::SingleCell(SingleCell { vis, .. }) => {
                let new_vis = vis.move_by(axis, amount); // Should check upper bounds
                cell_demap
                    .get_log_cell(&new_vis)
                    .map(|log| Self::SingleCell(SingleCell::new(new_vis, log)))
            }
            Self::SingleSlice(slice) => {
                let new_vis = slice.focus.vis.move_by(axis, amount);
                cell_demap.get_log_cell(&new_vis).map(|log| {
                    Self::SingleSlice(SingleSlice::new(slice.axis, SingleCell::new(new_vis, log)))
                })
            }
            Self::CellRange(CellRange { focus, .. }) => {
                let new_vis = focus.vis.move_by(axis, amount);
                cell_demap
                    .get_log_cell(&new_vis)
                    .map(|log| Self::SingleCell(SingleCell::new(new_vis, log)))
            }
            Self::SliceRange(SliceRange { axis, range }) => {
                let new_vis = range.focus.vis.move_by(*axis, amount);
                cell_demap.get_log_cell(&new_vis).map(|log| {
                    Self::SingleSlice(SingleSlice::new(
                        axis.clone(),
                        SingleCell::new(new_vis, log),
                    ))
                })
            }
        }
    }

    pub fn move_extent(&self, sel: TableSelection) -> Option<TableSelection> {
        let res = match (self, &sel) {
            (Self::SingleCell(cur), Self::SingleCell(ext)) => {
                Some(Self::CellRange(CellRange::new(cur.clone(), ext.clone())))
            }
            (Self::CellRange(CellRange { focus, .. }), Self::SingleCell(ext)) => {
                Some(Self::CellRange(CellRange::new(focus.clone(), ext.clone())))
            }
            _ => None,
        };
        //log::info!("Move extent: \ncur :\n{:?}  \nextent:\n{:?} \nresult:\n{:?}", self, sel, res);
        res
    }

    pub fn extend_in_axis(&mut self, axis: TableAxis, vis: VisIdx, cell_demap: &impl CellDemap) {
        if let Some(focus) = self.focus() {
            let vis_addr = AxisPair::new_for_axis(axis, vis, Default::default());

            if let Some(log_addr) = cell_demap.get_log_cell(&vis_addr) {
                *self = TableSelection::SliceRange(SliceRange {
                    axis: axis.clone(),
                    range: CellRange::new(focus.clone(), SingleCell::new(vis_addr, log_addr)),
                })
            }
        } else {
            self.select_in_axis(axis, vis, cell_demap)
        }
    }

    pub fn select_in_axis(&mut self, axis: TableAxis, vis: VisIdx, cell_demap: &impl CellDemap) {
        let vis_addr = AxisPair::new_for_axis(axis, vis, Default::default());
        if let Some(log_addr) = cell_demap.get_log_cell(&vis_addr) {
            *self = TableSelection::SingleSlice(SingleSlice::new(
                axis,
                SingleCell::new(vis_addr, log_addr),
            ))
        }
    }

    pub fn extend_from_focus_in_axis(
        &self,
        axis: &TableAxis,
        cell_demap: &impl CellDemap,
    ) -> Option<TableSelection> {
        // TODO: handle width of ranges and extend all of the cross axis that is covered
        self.vis_focus()
            .map(|vis_focus| {
                cell_demap.get_log_cell(vis_focus).map(|log_focus| {
                    TableSelection::SingleSlice(SingleSlice::new(
                        *axis,
                        SingleCell::new(vis_focus.clone(), log_focus),
                    ))
                })
            })
            .flatten()
    }

    pub fn add_selection(&self, sel: TableSelection) -> Option<TableSelection> {
        // Todo selection layers
        Some(sel)
    }

    pub fn has_focus(&self) -> bool {
        if let Self::NoSelection = self {
            false
        } else {
            true
        }
    }

    pub fn focus(&self) -> Option<&SingleCell> {
        match self {
            Self::NoSelection => None,
            Self::SingleCell(sc) => Some(sc),
            Self::SingleSlice(SingleSlice { focus, .. }) => Some(focus),
            Self::CellRange(CellRange { focus, .. }) => Some(focus),
            Self::SliceRange(SliceRange {
                range: CellRange { focus, .. },
                ..
            }) => Some(focus),
        }
    }

    pub fn vis_focus(&self) -> Option<&AxisPair<VisIdx>> {
        self.focus().map(|x| &x.vis)
    }

    pub fn to_axis_selection(
        &self,
        for_axis: TableAxis,
        _cell_demap: &impl CellDemap,
    ) -> IndicesSelection {
        match self {
            Self::NoSelection => IndicesSelection::NoSelection,
            Self::SingleCell(sc) => IndicesSelection::Single(sc.vis[for_axis]),
            Self::SingleSlice(SingleSlice { axis, focus }) => {
                if for_axis == *axis {
                    IndicesSelection::Single(focus.vis[*axis])
                } else {
                    IndicesSelection::NoSelection
                }
            }
            Self::CellRange(CellRange { focus, extent }) => IndicesSelection::Range {
                focus: focus.vis[for_axis],
                extent: extent.vis[for_axis],
            },
            Self::SliceRange(SliceRange {
                axis,
                range: CellRange { focus, extent },
            }) => {
                if for_axis == *axis {
                    IndicesSelection::Range {
                        focus: focus.vis[*axis],
                        extent: extent.vis[*axis],
                    }
                } else {
                    IndicesSelection::NoSelection
                }
            }
        }
    }

    pub fn get_drawable_selections(&self, bounding: &CellRect) -> DrawableSelections {
        match &self {
            TableSelection::SingleCell(sc) if bounding.contains_cell(&sc.vis) => {
                DrawableSelections::new(Some(sc.vis.clone()), Default::default())
            }
            TableSelection::SingleSlice(sl)
                if bounding.contains_idx(sl.axis, sl.focus.vis[sl.axis]) =>
            {
                DrawableSelections::new(
                    Some(sl.focus.vis.clone()),
                    vec![sl.to_cell_rect(bounding.range(sl.axis.cross_axis()))],
                )
            }
            TableSelection::CellRange(CellRange { focus, extent }) => {
                let row =
                    VisIdx::ascending(focus.vis[TableAxis::Rows], extent.vis[TableAxis::Rows]);
                let col = VisIdx::ascending(
                    focus.vis[TableAxis::Columns],
                    extent.vis[TableAxis::Columns],
                );

                let cell_rect = CellRect::new(row, col);

                //TODO: Intersection with bounding box
                DrawableSelections::new(Some(focus.vis), vec![cell_rect])
            }
            TableSelection::SliceRange(sr)
                if bounding.contains_idx(sr.axis, sr.range.focus.vis[sr.axis])
                    || bounding.contains_idx(sr.axis, sr.range.extent.vis[sr.axis]) =>
            {
                DrawableSelections::new(
                    Some(sr.range.focus.vis),
                    vec![sr.to_cell_rect(bounding.range(sr.axis.cross_axis()))],
                )
            }
            _ => DrawableSelections::new(None, Default::default()),
        }
    }

    pub fn fully_selects_heading(&self, in_axis: TableAxis, idx: VisIdx) -> bool {
        match self {
            TableSelection::SingleSlice(SingleSlice { axis, focus })
                if in_axis == *axis && focus.vis[*axis] == idx =>
            {
                true
            }
            TableSelection::SliceRange(SliceRange { axis, range })
                if in_axis == *axis && range.contains(in_axis, idx) =>
            {
                true
            }
            _ => false,
        }
    }

    pub fn fully_selected_on_axis(&self, in_axis: TableAxis) -> Option<Vec<VisIdx>> {
        match self {
            TableSelection::SingleSlice(SingleSlice { axis, focus }) if in_axis == *axis => {
                Some(vec![focus.vis[in_axis]])
            }
            TableSelection::SliceRange(SliceRange { axis, range }) if in_axis == *axis => {
                let (a, b) = VisIdx::ascending(range.focus.vis[in_axis], range.extent.vis[in_axis]);
                Some(VisIdx::range_inc_iter(a, b).collect())
            }
            _ => None,
        }
    }
}

impl From<SingleCell> for TableSelection {
    fn from(sc: SingleCell) -> Self {
        TableSelection::SingleCell(sc)
    }
}
