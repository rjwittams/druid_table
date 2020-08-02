use crate::axis_measure::{LogIdx, TableAxis, VisIdx};
use druid::{EventCtx, Selector};
use std::fmt::Debug;

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct CellAddress<T: Copy + Debug> {
    row: T,
    col: T,
}

impl<T: Copy + Debug> CellAddress<T> {
    pub(crate) fn new(row: T, col: T) -> CellAddress<T> {
        CellAddress { row, col }
    }
    fn main(&self, axis: TableAxis) -> T {
        match axis {
            TableAxis::Rows => self.row,
            TableAxis::Columns => self.col,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SingleCell {
    pub vis: CellAddress<VisIdx>,
    pub log: CellAddress<LogIdx>,
}

impl SingleCell {
    pub fn new(vis: CellAddress<VisIdx>, log: CellAddress<LogIdx>) -> Self {
        SingleCell { vis, log }
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
    //  SingleColumn
    //  SingleRow
    //  Range
    //  Discontiguous
}

#[derive(Debug, PartialEq)]
pub enum SelectionStatus {
    NotSelected,
    Primary,
    AlsoSelected,
}

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
    pub fn to_axis_selection(&self, axis: TableAxis) -> IndicesSelection {
        match self {
            TableSelection::NoSelection => IndicesSelection::NoSelection,
            TableSelection::SingleCell(sc) => {
                IndicesSelection::Single(sc.vis.main(axis), sc.log.main(axis))
            }
        }
    }

    pub(crate) fn get_cell_status(&self, address: CellAddress<VisIdx>) -> SelectionStatus {
        match self {
            TableSelection::SingleCell(sc) if address == sc.vis => SelectionStatus::Primary,
            _ => SelectionStatus::NotSelected,
        }
    }
}

pub const SELECT_INDICES: Selector<IndicesSelection> =
    Selector::new("druid-builtin.table.select-indices");

pub type SelectionHandler = dyn Fn(&mut EventCtx, &TableSelection);
