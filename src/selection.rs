use crate::axis_measure::{AxisPair, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::Remap;
use druid::{EventCtx, Selector};
use std::fmt::Debug;
use std::ops::{Add, Index, IndexMut};

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
                    .map(|log| TableSelection::SingleCell(SingleCell::new(vis_origin, log)))
            }
            Self::SingleCell(SingleCell { vis, .. }) => {
                let new_vis = vis.move_by(axis, amount);
                cell_demap
                    .get_log_cell(&new_vis)
                    .map(|log| TableSelection::SingleCell(SingleCell::new(new_vis, log)))
            }
            Self::SingleSlice(slice) => Some(self.clone()),
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
}

#[derive(Debug, PartialEq)]
pub enum SelectionStatus {
    NotSelected,
    Primary,
    AlsoSelected,
}

// TODO delete
impl From<SelectionStatus> for bool {
    fn from(ss: SelectionStatus) -> Self {
        ss != SelectionStatus::NotSelected
    }
}

impl From<SingleCell> for TableSelection {
    fn from(sc: SingleCell) -> Self {
        TableSelection::SingleCell(sc)
    }
}

impl TableSelection {
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

    pub(crate) fn get_cell_status(&self, address: &AxisPair<VisIdx>) -> SelectionStatus {
        match self {
            TableSelection::SingleCell(sc) if address == &sc.vis => SelectionStatus::Primary,
            TableSelection::SingleSlice(SingleSlice {
                focus: SingleCell { vis, .. },
                axis,
            }) => {
                if vis == address {
                    SelectionStatus::Primary
                } else if vis[axis] == address[axis] {
                    SelectionStatus::AlsoSelected
                } else {
                    SelectionStatus::NotSelected
                }
            }
            _ => SelectionStatus::NotSelected,
        }
    }
}

pub const SELECT_INDICES: Selector<IndicesSelection> =
    Selector::new("druid-builtin.table.select-indices");

pub type SelectionHandler = dyn Fn(&mut EventCtx, &TableSelection);
