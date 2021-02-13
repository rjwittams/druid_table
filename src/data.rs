use crate::axis_measure::{LogIdx, VisIdx, VisOffset};
use crate::data::SortDirection::Descending;
use druid::im::HashMap;
use druid::im::Vector;
use druid::Data;
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::ops::Range;
use std::hash::Hash;
use crate::data::IndexedDataOp::{Delete, Insert, Update, Move, MoveUpdate};
use std::time::Instant;
use std::sync::Arc;
use itertools::Either;

// This ended up sort of similar to Lens,
// so I've named the methods similarly.
// But it is implemented by the data itself
pub trait IndexedData: Data {
    type Item: Data;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&Self::Item) -> V) -> Option<V>;

    fn with_mut<V>(&mut self, idx: LogIdx, f: impl FnOnce(&mut Self::Item) -> V) -> Option<V>;
    // Seems advisable not to clash with len
    fn data_len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.data_len() == 0
    }
}

impl<RowData: Data> IndexedData for Vector<RowData> {
    type Item = RowData;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        let option = self.get(idx.0);
        option.map(f)
    }

    fn with_mut<V>(&mut self, idx: LogIdx, f: impl FnOnce(&mut Self::Item) -> V) -> Option<V> {
        if let Some(found) = self.get(idx.0){
            let mut tmp = found.clone();
            let ret = f(&mut tmp);
            if !tmp.same(found){
                self[idx.0] = tmp;
            }
            Some(ret)
        }else{
            None
        }
    }

    fn data_len(&self) -> usize {
        Vector::len(self)
    }
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum IndexedDataOp{
    Insert(LogIdx),
    Delete(LogIdx),
    Move(LogIdx, LogIdx),
    MoveUpdate(LogIdx, LogIdx),
    Update(LogIdx),
}

#[derive(PartialEq, Debug, Data, Clone)]
pub struct IndexedDataDiff{
    instant: Instant,
    #[data(ignore)]
    ops: Option<Arc<[IndexedDataOp]>> // If this is None, then its a full refresh
}

impl IndexedDataDiff {
    pub fn new(ops: Vec<IndexedDataOp>) -> Self {
        let ops: Arc<[IndexedDataOp]> = ops.into();
        IndexedDataDiff { instant: Instant::now(), ops: ops.into() }
    }

    pub fn refresh() -> Self{
        IndexedDataDiff { instant: Instant::now(), ops: None }
    }

    pub fn is_refresh(&self) -> bool{
        self.ops.is_none()
    }

    pub fn ops(&self) -> impl Iterator<Item=IndexedDataOp> + '_{
        if let Some(ops) =  &self.ops{
            ops.iter().cloned()
        }else{
            [].iter().cloned()
        }
    }
}


pub trait IndexedDataDiffer<T: IndexedData>{
    fn diff(&self, old: &T, new: &T)->Option<IndexedDataDiff>;
}

pub struct RefreshDiffer;

impl <T: IndexedData> IndexedDataDiffer<T> for RefreshDiffer {
    fn diff(&self, old: &T, new: &T) -> Option<IndexedDataDiff> {
         if old.same(new){
            None
        }else{
            Some(IndexedDataDiff::refresh())
        }
    }
}

pub struct SlowVectorDiffer<F>{
    key_extract: F
}

impl<F> SlowVectorDiffer<F> {
    pub fn new<T, K>(key_extract: F) -> Self where T: Data, F: Fn(&T)->K, K: Hash + Eq + Clone {
        SlowVectorDiffer { key_extract }
    }
}


impl <T: Data, F: Fn(&T)->K, K: Hash + Eq + Clone> IndexedDataDiffer<Vector<T>> for SlowVectorDiffer<F> {
    fn diff(&self, old: &Vector<T>, new: &Vector<T>) -> Option<IndexedDataDiff> {
        if old.same(new){
            None
        }else {
            let key_extract = &self.key_extract;
            let mut old_indexes: HashMap::<_, _> = old.iter().enumerate()
                .map(move |(idx, item)| (key_extract(item), idx)).collect();

            let key_extract = &self.key_extract;
            let mut ops = Vec::<IndexedDataOp>::new();
            for (new_idx, new_item) in new.iter().enumerate() {
                let key = key_extract(new_item);
                if let Some(old_idx) = old_indexes.remove(&key) {
                    let old_item = &old[old_idx];
                    let updated = !old_item.same(new_item);
                    let moved = old_idx != new_idx;
                    match (updated, moved) {
                        (false, false) => (),
                        (false, true) => ops.push(Move(LogIdx(old_idx), LogIdx(new_idx))),
                        (true, true) => ops.push(MoveUpdate(LogIdx(old_idx), LogIdx(old_idx))),
                        (true, false) => ops.push(Update(LogIdx(new_idx))),
                    }
                } else {
                    ops.push(Insert(LogIdx(new_idx)))
                }
            }
            ops.extend(old_indexes.into_iter().map(|(_, old_idx)| Delete(LogIdx(old_idx))));
            Some(IndexedDataDiff::new(ops))
        }
    }
}

#[derive(Clone, Data, Debug)]
pub enum RemapDetails {
    Full(Vector<LogIdx>), // Could do versioning for sameness if Vector is an issue
}

impl RemapDetails {
    fn get_log_idx(&self, idx: VisIdx) -> Option<LogIdx> {
        match self {
            RemapDetails::Full(v) => v.get(idx.0).cloned(),
        }
    }
}

impl Remap {
    pub fn new() -> Remap {
        Remap::Pristine
    }

    pub fn is_pristine(&self) -> bool {
        if let Remap::Pristine = self {
            true
        } else {
            false
        }
    }

    pub fn max_vis_idx(&self, len: usize) -> VisIdx {
        if let Remap::Selected(RemapDetails::Full(v)) = self {
            VisIdx(v.len()) + VisOffset(-1)
        } else {
            VisIdx(len) + VisOffset(-1)
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
            Remap::Selected(v) => v.get_log_idx(vis_idx),
            _ => Some(LogIdx(vis_idx.0)), // Dunno if right for internal
        }
    }

    pub fn iter(&self, last: VisIdx)->impl Iterator<Item=LogIdx>+ '_{
        VisIdx::range_inc_iter(VisIdx(0), last)
            .map(move |vis|self.get_log_idx(vis).unwrap_or_else(||LogIdx(vis.0)))
    }
}

impl Default for Remap {
    fn default() -> Self {
        Remap::Pristine
    }
}

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
            sort_by: Vector::default()
        }
    }
}

pub trait Remapper<TableData: IndexedData> {
    fn sort_fixed(&self, idx: usize) -> bool;
    fn initial_spec(&self) -> RemapSpec;
    // This takes our normal data and a spec, and returns a remap of its indices
    fn remap_from_records(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap;
}


#[cfg(test)]
mod test{
    use crate::data::{SlowVectorDiffer, IndexedDataDiffer, IndexedDataDiff};
    use druid::im::Vector;
    use crate::data::IndexedDataOp::*;
    use crate::LogIdx;

    #[test]
    fn test_slow_vec_differ(){
        let differ = SlowVectorDiffer::new(|s:&(char, usize)|s.0);

        let diff = differ.diff(
            &vec![('A', 1usize), ('B', 2), ('C', 3), ('D', 5), ('T', 23)].into_iter().collect(),
            &vec![('A', 1usize), ('B', 10), ('Z', 23), ('I', 13), ('D', 5), ('T', 12)].into_iter().collect()
        );

        assert_eq!(IndexedDataDiff::Ops(vec![Update(LogIdx(1)),
                                             Insert(LogIdx(2)),
                                             Insert(LogIdx(3)),
                                             Move(LogIdx(3), LogIdx(4)),
                                             MoveUpdate(LogIdx(4), LogIdx(4)),
                                             Delete(LogIdx(2))]),
                   diff)
    }


}