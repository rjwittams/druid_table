use crate::axis_measure::{LogIdx, VisIdx};
use druid::im::Vector;
use druid::Data;

// This ended up sort of similar to Lens,
// so I've named the methods similarly.
// But it is implemented by the data itself
pub trait IndexedItems {
    type Item;
    type Idx: Copy + Ord; // + Into<usize>?
                          // This takes a callback so it can work
                          // the same way for concrete and virtual data sources
                          // but still provide a reference.
    fn with<V>(&self, idx: Self::Idx, f: impl FnOnce(&Self::Item) -> V) -> Option<V>;

    fn with_mut<V>(&mut self, idx: Self::Idx, f: impl FnOnce(&mut Self::Item) -> V) -> Option<V>;
    // Seems advisable not to clash with len
    fn idx_len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.idx_len() == 0
    }
}

pub trait IndexedData: IndexedItems + Data
where
    <Self as IndexedItems>::Item: Data,
{
}

impl<T> IndexedData for T
where
    T: IndexedItems + Data,
    T::Item: Data,
{
}

impl<RowData: Clone> IndexedItems for Vector<RowData> {
    type Item = RowData;
    type Idx = LogIdx;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        let option = self.get(idx.0);
        option.map(f)
    }

    fn with_mut<V>(&mut self, idx: Self::Idx, f: impl FnOnce(&mut Self::Item) -> V) -> Option<V> {
        let option = self.get_mut(idx.0);
        option.map(f)
    }

    fn idx_len(&self) -> usize {
        Vector::len(self)
    }
}

impl<RowData> IndexedItems for Vec<RowData> {
    type Item = RowData;
    type Idx = LogIdx;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx.0).map(f)
    }

    fn with_mut<V>(&mut self, idx: Self::Idx, f: impl FnOnce(&mut Self::Item) -> V) -> Option<V> {
        self.get_mut(idx.0).map(f)
    }

    fn idx_len(&self) -> usize {
        Vec::len(self)
    }
}

#[derive(Clone, Data, Debug)]
pub enum RemapDetails {
    Full(Vector<LogIdx>), // Could do versioning for sameness if Vector is an issue
}

impl RemapDetails {
    fn get_log_idx(&self, idx: VisIdx) -> Option<&LogIdx> {
        match self {
            RemapDetails::Full(v) => v.get(idx.0),
        }
    }
}

#[derive(Debug, Data, Clone)]
pub enum Remap {
    Pristine,
    Selected(RemapDetails),
    Internal, // This indicates that the source data has done the remapping, ie no wrapper required. Eg sort in db.
              //  need some token to give back to the table rows
}

impl Remap {
    pub fn get_log_idx(&self, vis_idx: VisIdx) -> Option<LogIdx> {
        match self {
            Remap::Selected(v) => v.get_log_idx(vis_idx).cloned(),
            _ => Some(LogIdx(vis_idx.0)), // Dunno if right for internal
        }
    }
}

impl Default for Remap {
    fn default() -> Self {
        Remap::Pristine
    }
}

use crate::data::SortDirection::Descending;
use std::cmp::Ordering;

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Copy, Data)]
pub enum SortDirection {
    Ascending,
    Descending,
}
impl SortDirection {
    pub fn apply(&self, ord: Ordering) -> Ordering {
        match self {
            Descending => ord.reverse(),
            _ => ord,
        }
    }
}
#[derive(Clone, Debug, Data)]
pub struct SortSpec {
    pub(crate) idx: usize,
    //TODO: This index is used in two different ways... the index of the original column (should be log idx) or the sort order.
    // This is a bit weird maybe parameterize ..
    pub(crate) direction: SortDirection,
}

impl SortSpec {
    pub fn new(idx: usize, direction: SortDirection) -> Self {
        SortSpec { idx, direction }
    }

    pub fn descending(mut self) -> Self {
        self.direction = SortDirection::Descending;
        self
    }
}

#[derive(Clone, Debug, Data)]
pub struct RemapSpec {
    pub(crate) sort_by: Vector<SortSpec>, // columns sorted
                                          // filters
                                          // Explicit moves
}

impl RemapSpec {
    pub(crate) fn add_sort(&mut self, s: SortSpec) {
        self.sort_by.push_back(s)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sort_by.is_empty()
    }

    pub(crate) fn toggle_sort(&mut self, log_idx: LogIdx, extend: bool) -> bool {
        let sort_by = &mut self.sort_by;
        let log_idx = log_idx.0;

        match sort_by.last() {
            Some(SortSpec { idx, direction }) if log_idx == *idx => {
                let dir = direction.clone();
                sort_by.pop_back();
                if dir == SortDirection::Ascending {
                    sort_by.push_back(SortSpec::new(log_idx, SortDirection::Descending));
                }
            }
            _ => {
                if !extend {
                    sort_by.clear();
                }
                sort_by.push_back(SortSpec::new(log_idx, SortDirection::Ascending));
            }
        }
        // Handle sorting disabled for a column
        true
    }
}

impl Default for RemapSpec {
    fn default() -> Self {
        RemapSpec {
            sort_by: Vector::default(),
        }
    }
}

pub trait Remapper<TableData: IndexedData>
where
    TableData::Item: Data,
{
    // This takes our normal data and a spec, and returns a remapped view of it if required
    fn sort_fixed(&self, idx: usize) -> bool;
    fn initial_spec(&self) -> RemapSpec;
    fn remap_items(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap;
}
