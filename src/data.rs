use crate::axis_measure::{LogIdx, VisIdx, VisOffset};
use crate::data::IndexedDataOp::{Delete, Insert, Move, Update};
use crate::data::SortDirection::Descending;
use druid::im::HashMap;
use druid::im::Vector;
use druid::Data;
use std::cmp::Ordering;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Instant;

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
        if let Some(found) = self.get(idx.0) {
            let mut tmp = found.clone();
            let ret = f(&mut tmp);
            if !tmp.same(found) {
                self[idx.0] = tmp;
            }
            Some(ret)
        } else {
            None
        }
    }

    fn data_len(&self) -> usize {
        Vector::len(self)
    }
}

#[derive(Data, PartialEq, Debug, Copy, Clone)]
pub enum IndexedDataOp {
    Insert(LogIdx),
    Delete(LogIdx),
    Move(LogIdx, LogIdx),
    Update(LogIdx),
}

#[derive(PartialEq, Debug, Data, Clone)]
pub struct IndexedDataDiff {
    instant: Instant,
    #[data(ignore)]
    ops: Option<Arc<[IndexedDataOp]>>, // If this is None, then its a full refresh
}

impl IndexedDataDiff {
    pub fn new(ops: Vec<IndexedDataOp>) -> Self {
        let ops: Arc<[IndexedDataOp]> = ops.into();
        IndexedDataDiff {
            instant: Instant::now(),
            ops: ops.into(),
        }
    }

    pub fn refresh() -> Self {
        IndexedDataDiff {
            instant: Instant::now(),
            ops: None,
        }
    }

    pub fn is_refresh(&self) -> bool {
        self.ops.is_none()
    }

    pub fn ops(&self) -> impl Iterator<Item = IndexedDataOp> + '_ {
        if let Some(ops) = &self.ops {
            ops.iter().cloned()
        } else {
            [].iter().cloned()
        }
    }
}

pub trait IndexedDataDiffer<T: IndexedData> {
    fn diff(&self, old: &T, new: &T) -> Option<IndexedDataDiff>;
}

pub struct RefreshDiffer;

impl<T: IndexedData> IndexedDataDiffer<T> for RefreshDiffer {
    fn diff(&self, old: &T, new: &T) -> Option<IndexedDataDiff> {
        if old.same(new) {
            None
        } else {
            Some(IndexedDataDiff::refresh())
        }
    }
}

pub struct SlowVectorDiffer<F> {
    key_extract: F,
}

impl<F> SlowVectorDiffer<F> {
    pub fn new<T, K>(key_extract: F) -> Self
    where
        T: Data,
        F: Fn(&T) -> K,
        K: Hash + Eq + Clone,
    {
        SlowVectorDiffer { key_extract }
    }
}

impl<T: Data, F: Fn(&T) -> K, K: Hash + Eq + Clone> IndexedDataDiffer<Vector<T>>
    for SlowVectorDiffer<F>
{
    fn diff(&self, old: &Vector<T>, new: &Vector<T>) -> Option<IndexedDataDiff> {
        if old.same(new) {
            None
        } else {
            let key_extract = &self.key_extract;
            let mut old_indexes: std::collections::HashMap<_, _> = old
                .iter()
                .enumerate()
                .map(move |(idx, item)| (key_extract(item), idx))
                .collect();

            let mut ops = Vec::<IndexedDataOp>::new();
            for (new_idx, new_item) in new.iter().enumerate() {
                let key = (self.key_extract)(new_item);
                if let Some(old_idx) = old_indexes.remove(&key) {
                    if old_idx != new_idx {
                        ops.push(Move(LogIdx(old_idx), LogIdx(new_idx)));
                    }

                    let old_item = &old[old_idx];
                    if !old_item.same(new_item) {
                        ops.push(Update(LogIdx(new_idx)))
                    }
                } else {
                    ops.push(Insert(LogIdx(new_idx)))
                }
            }
            ops.extend(
                old_indexes
                    .into_iter()
                    .map(|(_, old_idx)| Delete(LogIdx(old_idx))),
            );
            Some(IndexedDataDiff::new(ops))
        }
    }
}

#[derive(Clone, Data, Debug)]
pub enum RemapDetails {
    Full(Vector<LogIdx>, HashMap<LogIdx, VisIdx>),
    // Could do versioning/stamping for sameness if immutable collections an issue
}

impl RemapDetails {
    pub(crate) fn make_full(vis_to_log: impl IntoIterator<Item = LogIdx>) -> RemapDetails {
        let vis_to_log: Vector<LogIdx> = vis_to_log.into_iter().collect();
        let log_to_vis = vis_to_log
            .iter()
            .enumerate()
            .map(|(v, log)| (*log, VisIdx(v)))
            .collect();
        RemapDetails::Full(vis_to_log, log_to_vis)
    }

    fn get_log_idx(&self, vis: VisIdx) -> Option<LogIdx> {
        match self {
            RemapDetails::Full(vis_to_log, _) => vis_to_log.get(vis.0).cloned(),
        }
    }

    fn get_vis_idx(&self, log: LogIdx) -> Option<VisIdx> {
        match self {
            RemapDetails::Full(_, log_to_vis) => log_to_vis.get(&log).cloned(),
        }
    }
}

impl Remap {
    pub fn new(len: usize) -> Remap {
        Remap::Pristine(len)
    }

    pub fn is_pristine(&self) -> bool {
        if let Remap::Pristine(_) = self {
            true
        } else {
            false
        }
    }

    pub fn max_vis_idx(&self, len: usize) -> VisIdx {
        if let Remap::Selected(RemapDetails::Full(v, _)) = self {
            VisIdx(v.len()) + VisOffset(-1)
        } else {
            VisIdx(len) + VisOffset(-1)
        }
    }
}

#[derive(Debug, Data, Clone)]
pub enum Remap {
    Pristine(usize),
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

    pub fn get_vis_idx(&self, log_idx: LogIdx) -> Option<VisIdx> {
        match self {
            Remap::Selected(v) => v.get_vis_idx(log_idx),
            _ => Some(VisIdx(log_idx.0)),
        }
    }

    pub fn log_idx_in_vis_order(&self, last: VisIdx) -> impl Iterator<Item = LogIdx> + '_ {
        VisIdx::range_inc_iter(VisIdx(0), last)
            .map(move |vis| self.get_log_idx(vis).unwrap_or_else(|| LogIdx(vis.0)))
    }
}

impl Default for Remap {
    fn default() -> Self {
        Remap::Pristine(0)
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
            sort_by: Vector::default(),
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
mod test {
    use crate::data::IndexedDataOp::*;
    use crate::data::{IndexedDataDiff, IndexedDataDiffer, SlowVectorDiffer};
    use crate::LogIdx;
    use druid::im::Vector;

    #[test]
    fn test_slow_vec_differ() {
        let differ = SlowVectorDiffer::new(|s: &(char, usize)| s.0);

        let diff = differ.diff(
            &vec![('A', 1usize), ('B', 2), ('C', 3), ('D', 5), ('T', 23)]
                .into_iter()
                .collect(),
            &vec![
                ('A', 1usize),
                ('B', 10),
                ('Z', 23),
                ('I', 13),
                ('D', 5),
                ('T', 12),
            ]
            .into_iter()
            .collect(),
        ).expect("diff exists");

        assert_eq!(
            IndexedDataDiff::new(vec![
                Update(LogIdx(1)),
                Insert(LogIdx(2)),
                Insert(LogIdx(3)),
                Move(LogIdx(3), LogIdx(4)),
                Move(LogIdx(4), LogIdx(4)),
                Update(LogIdx(4)),
                Delete(LogIdx(2))
            ]),
            diff
        )
    }
}
