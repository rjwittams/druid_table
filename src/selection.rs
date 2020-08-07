use crate::axis_measure::{AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use std::fmt::Debug;
use std::iter::Map;
use std::ops::{Add, Index, IndexMut, RangeInclusive};

// Could be the address of a cell or something else we have one of for each axis
impl<T: Copy + Debug + Default> AxisPair<T> {
    pub fn new(row: T, col: T) -> AxisPair<T> {
        AxisPair { row, col }
    }

    pub fn new_for_axis(axis: &TableAxis, main: T, cross: T) -> AxisPair<T> {
        let mut ca = AxisPair::new(Default::default(), Default::default());
        ca[axis] = main;
        ca[axis.cross_axis()] = cross;
        ca
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

    // Todo impl Iterator
    pub fn rows(&self) -> Map<RangeInclusive<usize>, fn(usize) -> VisIdx> {
        VisIdx::range_inc_iter(self.start_row, self.end_row) // Todo work out how to support custom range
    }

    pub fn cols(&self) -> Map<RangeInclusive<usize>, fn(usize) -> VisIdx> {
        VisIdx::range_inc_iter(self.start_col, self.end_col)
    }

    fn contains_cell(&self, cell_addr: &AxisPair<VisIdx>) -> bool {
        self.contains_idx(&TableAxis::Columns, cell_addr.col)
            && self.contains_idx(&TableAxis::Rows, cell_addr.row)
    }

    fn range(&self, axis: &TableAxis) -> (VisIdx, VisIdx) {
        match axis {
            TableAxis::Rows => (self.start_row, self.end_row),
            TableAxis::Columns => (self.start_col, self.end_col),
        }
    }

    fn contains_idx(&self, axis: &TableAxis, idx: VisIdx) -> bool {
        let (start, end) = self.range(axis);
        start <= idx && end >= idx
    }
}

trait AxisPairMove<O> {
    fn move_by(&self, axis: &TableAxis, amount: O) -> Self;
}

impl<O, T: Add<O, Output = T> + Copy + Debug + Default> AxisPairMove<O> for AxisPair<T> {
    fn move_by(&self, axis: &TableAxis, amount: O) -> AxisPair<T> {
        let mut moved = (*self).clone();
        moved[axis] = self[axis] + amount;
        moved
    }
}

impl<T: Copy + Debug + Default> Index<&TableAxis> for AxisPair<T> {
    type Output = T;

    fn index(&self, axis: &TableAxis) -> &Self::Output {
        match axis {
            TableAxis::Rows => &self.row,
            TableAxis::Columns => &self.col,
        }
    }
}

impl<T: Copy + Debug + Default> IndexMut<&TableAxis> for AxisPair<T> {
    fn index_mut(&mut self, axis: &TableAxis) -> &mut Self::Output {
        match axis {
            TableAxis::Rows => &mut self.row,
            TableAxis::Columns => &mut self.col,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
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
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SingleSlice {
    pub axis: TableAxis,
    pub focus: SingleCell, // The cell we are focused on, that determines the slice
}

impl SingleSlice {
    pub fn new(axis: TableAxis, focus: SingleCell) -> Self {
        SingleSlice { axis, focus }
    }

    pub fn to_cell_rect(&self, (cross_s, cross_e): (VisIdx, VisIdx)) -> CellRect {
        let main = self.focus.vis[&self.axis];
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
    Single(VisIdx, LogIdx),
    //Many(Vec<usize>),
    //Range(from, to)
}

impl IndicesSelection {
    pub(crate) fn vis_index_selected(&self, vis_idx: VisIdx) -> bool {
        match self {
            IndicesSelection::Single(sel_vis, _) => *sel_vis == vis_idx,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TableSelection {
    NoSelection,
    SingleCell(SingleCell),
    SingleSlice(SingleSlice),
    //  CellRange
    //  SliceRange
    //  Discontiguous
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
        axis: &TableAxis,
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
                    Self::SingleSlice(SingleSlice::new(
                        slice.axis,
                        SingleCell::new(new_vis, log),
                    ))
                })
            }
        }
    }

    pub fn extend_in_axis(
        &self,
        axis: TableAxis,
        cell_demap: &impl CellDemap,
    ) -> Option<TableSelection> {
        // TODO: handle width of ranges and extend all of the cross axis that is covered
        self.focus()
            .map(|vis_focus| {
                cell_demap.get_log_cell(vis_focus).map(|log_focus| {
                    TableSelection::SingleSlice(SingleSlice::new(
                        axis,
                        SingleCell::new(vis_focus.clone(), log_focus),
                    ))
                })
            })
            .flatten()
    }

    pub fn focus(&self) -> Option<&AxisPair<VisIdx>> {
        match self {
            Self::NoSelection => None,
            Self::SingleCell(SingleCell { vis, .. }) => Some(vis),
            Self::SingleSlice(SingleSlice { focus, .. }) => Some(&focus.vis),
        }
    }

    pub fn to_axis_selection(&self, for_axis: &TableAxis) -> IndicesSelection {
        match self {
            Self::NoSelection => IndicesSelection::NoSelection,
            Self::SingleCell(sc) => IndicesSelection::Single(sc.vis[for_axis], sc.log[for_axis]),
            Self::SingleSlice(SingleSlice { axis, focus }) => {
                if for_axis == axis {
                    IndicesSelection::Single(focus.vis[axis], focus.log[axis])
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
                if bounding.contains_idx(&sl.axis, sl.focus.vis[&sl.axis]) =>
            {
                DrawableSelections::new(
                    Some(sl.focus.vis.clone()),
                    vec![sl.to_cell_rect(bounding.range(sl.axis.cross_axis()))],
                )
            }
            _ => DrawableSelections::new(None, Default::default()),
        }
    }
}

impl From<SingleCell> for TableSelection {
    fn from(sc: SingleCell) -> Self {
        TableSelection::SingleCell(sc)
    }
}



