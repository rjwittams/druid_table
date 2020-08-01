use druid::im::Vector;
use druid::Data;

pub trait ItemsLen {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait ItemsUse: ItemsLen {
    type Item: Data;
    // This takes a callback so it can work
    // the same way for concrete and virtual data sources
    // but still provide a reference.
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V>;

    // Test only as we never want to use in real code - could blow up memory
    #[cfg(test)]
    fn all_items(&self) -> Vector<Self::Item> {
        (0usize..self.len())
            .map(|i| self.use_item(i, |item| item.clone()))
            .flatten()
            .collect()
    }
}

pub trait TableRows: ItemsUse + Data {}

impl<T> TableRows for T where T: ItemsUse + Data {}

impl<T: Clone> ItemsLen for Vector<T> {
    fn len(&self) -> usize {
        Vector::len(self)
    }
}

impl<RowData: Data> ItemsUse for Vector<RowData> {
    type Item = RowData;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        let option = self.get(idx);
        log::info!("Using idx {} is_none{} len {}", idx, option.is_none(), self.len());
        option.map(move |x| f(x))
    }
}

impl<T> ItemsLen for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl<RowData: Data> ItemsUse for Vec<RowData> {
    type Item = RowData;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx).map(move |x| f(x))
    }
}

#[derive(Clone)] // TODO defeat the borrow checker so we don't have to do this
pub enum RemapDetails {
    Full(Vec<usize>), // TODO : Adaptive storage for
                      // the case where only a few remappings have been done, ie manual moves.
                      // Runs of shifts in a Vec<(usize,usize)>
                      // Sorting and filtering will always provide a full vec
}

impl RemapDetails {
    fn get(&self, idx: usize) -> Option<&usize> {
        match self {
            RemapDetails::Full(v) => v.get(idx),
        }
    }
}

pub enum Remap {
    Pristine,
    Selected(RemapDetails),
    Internal, // This indicates that the source data has done the remapping, ie no wrapper required. Eg sort in db.
              //  need some token to give back to the table rows
}

use crate::data::SortDirection::Descending;
use std::cmp::Ordering;

pub struct RemappedItems<'a, 'b, U: ItemsUse> {
    pub(crate) underlying: &'a U,
    pub(crate) details: &'b RemapDetails,
}

impl<'a, 'b, U: ItemsUse> RemappedItems<'a, 'b, U> {
    pub fn new(underlying: &'a U, details: &'b RemapDetails) -> RemappedItems<'a, 'b, U> {
        RemappedItems {
            underlying,
            details,
        }
    }
}

impl<U: ItemsUse + Data> ItemsLen for RemappedItems<'_, '_, U> {
    fn len(&self) -> usize {
        let RemapDetails::Full(remap) = &self.details;
        remap.len()
    }
}

impl<U: ItemsUse + Data> ItemsUse for RemappedItems<'_, '_, U> {
    type Item = U::Item;

    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        self.details
            .get(idx)
            .and_then(|new_idx| self.underlying.use_item(*new_idx, f))
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

pub trait Remapper<RowData: Data, TableData: TableRows<Item = RowData>> {
    // This takes our normal data and a spec, and returns a remapped view of it if required
    fn sort_fixed(&self, idx: usize) -> bool;
    fn initial_spec(&self) -> RemapSpec;
    fn remap(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap;
}

#[cfg(test)]
mod test {
    use crate::data::*;
    use im::Vector;

    #[test]
    fn remap() {
        let und: Vector<usize> = (0usize..=10).into_iter().collect();
        assert_eq!(und, und.all_items());
        let remap_idxs = vec![8, 7, 5, 1];
        let details = RemapDetails::Full(remap_idxs.clone());
        let remapped = RemappedItems::new(&und, &details);
        // assert_eq!(Some(0), remapped.use_item(0, |u|*u));
        // assert_eq!(Some(7), remapped.use_item(7, |u|*u));
        // assert_eq!(und, remapped.all_items());

        let res: Vector<usize> = remap_idxs.into_iter().collect();
        assert_eq!(res, remapped.all_items());
    }
}
