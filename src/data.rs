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

    //fn with_mut<V>(&self, idx: Self::Idx, f: impl FnOnce(&Self::Item)) -> Option<V>;
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

impl<RowData: Data> IndexedItems for Vector<RowData> {
    type Item = RowData;
    type Idx = LogIdx;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        let option = self.get(idx.0);
        option.map(move |x| f(x))
    }
    fn idx_len(&self) -> usize {
        Vector::len(self)
    }
}

impl<RowData: Data> IndexedItems for Vec<RowData> {
    type Item = RowData;
    type Idx = LogIdx;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx.0).map(move |x| f(x))
    }

    fn idx_len(&self) -> usize {
        Vec::len(self)
    }
}

#[derive(Clone, Debug)]
pub enum RemapDetails {
    Full(Vec<LogIdx>), // TODO : Adaptive storage for
                       // the case where only a few remappings have been done, ie manual moves.
                       // Runs of shifts in a Vec<(usize,usize)>
                       // Sorting and filtering will always provide a full vec
}

impl RemapDetails {
    fn get_log_idx(&self, idx: VisIdx) -> Option<&LogIdx> {
        match self {
            RemapDetails::Full(v) => v.get(idx.0),
        }
    }
}

#[derive(Debug, Clone)]
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

use crate::data::SortDirection::Descending;
use std::cmp::Ordering;
use std::ops::{Add, Sub};

pub struct RemappedItems<'a, 'b, U: IndexedItems<Idx = LogIdx>> {
    pub(crate) underlying: &'a U,
    pub(crate) details: &'b RemapDetails,
}

impl<'a, 'b, U: IndexedItems<Idx = LogIdx>> RemappedItems<'a, 'b, U> {
    pub fn new(underlying: &'a U, details: &'b RemapDetails) -> RemappedItems<'a, 'b, U> {
        RemappedItems {
            underlying,
            details,
        }
    }
}

impl<U: IndexedItems<Idx = LogIdx> + Data> IndexedItems for RemappedItems<'_, '_, U> {
    type Item = U::Item;
    type Idx = VisIdx;

    fn with<V>(&self, idx: VisIdx, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        self.details
            .get_log_idx(idx)
            .and_then(|new_idx| self.underlying.with(*new_idx, f))
    }

    fn idx_len(&self) -> usize {
        let RemapDetails::Full(remap) = &self.details;
        remap.len()
    }
}
#[derive(Clone)]
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
#[derive(Clone)]
pub struct SortSpec {
    pub(crate) idx: usize, // must be the index in the underlying column order to work
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

pub struct RemapSpec {
    pub(crate) sort_by: Vec<SortSpec>, // columns sorted
                                       // filters
}

impl RemapSpec {
    pub(crate) fn clear(&mut self) {
        self.sort_by.clear()
    }

    pub(crate) fn add_sort(&mut self, s: SortSpec) {
        self.sort_by.push(s)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sort_by.is_empty()
    }
}

impl Default for RemapSpec {
    fn default() -> Self {
        RemapSpec {
            sort_by: Vec::default(),
        }
    }
}

pub trait Remapper<TableData: IndexedData> where TableData::Item : Data {
    // This takes our normal data and a spec, and returns a remapped view of it if required
    fn sort_fixed(&self, idx: usize) -> bool;
    fn initial_spec(&self) -> RemapSpec;
    fn remap(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap;
}

// #[cfg(test)]
// mod test {
//     use crate::data::*;
//     use im::Vector;
//
//     #[test]
//     fn remap() {
//         let und: Vector<usize> = (0usize..=10).into_iter().collect();
//         assert_eq!(und, und.all_items());
//         let remap_idxs = vec![8, 7, 5, 1];
//         let details = RemapDetails::Full(remap_idxs.clone());
//         let remapped = RemappedItems::new(&und, &details);
//         // assert_eq!(Some(0), remapped.use_item(0, |u|*u));
//         // assert_eq!(Some(7), remapped.use_item(7, |u|*u));
//         // assert_eq!(und, remapped.all_items());
//
//         let res: Vector<usize> = remap_idxs.into_iter().collect();
//         assert_eq!(res, remapped.all_items());
//     }
// }
