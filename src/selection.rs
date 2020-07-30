use crate::axis_measure::TableAxis;
use druid::{EventCtx, Selector};

#[derive(Debug, Clone)]
pub struct SingleCell {
    row: usize,
    col: usize,
}

impl SingleCell {
    pub(crate) fn new(row: usize, col: usize) -> SingleCell {
        SingleCell { row, col }
    }
    fn main(&self, axis: TableAxis) -> usize {
        match axis {
            TableAxis::Rows => self.row,
            TableAxis::Columns => self.col,
        }
    }
}

#[derive(Debug, Clone)]
pub enum IndicesSelection {
    NoSelection,
    Single(usize),
    //Many(Vec<usize>),
    //Range(from, to)
}

impl IndicesSelection {
    pub(crate) fn index_selected(&self, idx: usize) -> bool {
        match self {
            IndicesSelection::Single(sel_idx) if *sel_idx == idx => true,
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
            TableSelection::SingleCell(sc) => IndicesSelection::Single(sc.main(axis)),
        }
    }

    pub(crate) fn get_cell_status(&self, row_idx: usize, col_idx: usize) -> SelectionStatus {
        match self {
            TableSelection::SingleCell(sc) if row_idx == sc.row && col_idx == sc.col => {
                SelectionStatus::Primary
            }
            _ => SelectionStatus::NotSelected,
        }
    }
}

pub const SELECT_INDICES: Selector<IndicesSelection> =
    Selector::new("druid-builtin.table.select-indices");

pub type SelectionHandler = dyn Fn(&mut EventCtx, &TableSelection);
