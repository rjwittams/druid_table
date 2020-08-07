use crate::axis_measure::LogIdx;
use crate::IndexedItems;
use druid::{Data, Lens};

#[derive(Debug, Data, Clone, Lens)]
pub struct LogIdxTable {
    rows: usize,
}

impl LogIdxTable {
    pub fn new(rows: usize) -> Self {
        LogIdxTable { rows }
    }
}

impl IndexedItems for LogIdxTable {
    type Item = LogIdx;
    type Idx = LogIdx;
    fn with<V>(&self, idx: LogIdx, f: impl FnOnce(&LogIdx) -> V) -> Option<V> {
        if idx.0 < self.rows {
            Some(f(&idx))
        } else {
            None
        }
    }

    fn with_mut<V>(&mut self, _idx: Self::Idx, _f: impl FnOnce(&mut Self::Item)->V) -> Option<V> {
        None
    }

    fn idx_len(&self) -> usize {
        self.rows
    }
}
