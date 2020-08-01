use crate::{ItemsLen, ItemsUse};
use druid::{Data, Lens};
use crate::axis_measure::LogIdx;

#[derive(Debug, Data, Clone, Lens)]
pub struct LogIdxTable {
    rows: usize,
}

impl LogIdxTable {
    pub fn new(rows: usize) -> Self {
        LogIdxTable { rows }
    }
}

impl ItemsLen for LogIdxTable {
    fn len(&self) -> usize {
        self.rows
    }
}

impl ItemsUse for LogIdxTable {
    type Item = LogIdx;
    type Idx = LogIdx;
    fn use_item<V>(&self, idx: LogIdx, f: impl FnOnce(&LogIdx) -> V) -> Option<V> {
        if idx.0 < self.rows {
            Some(f(&idx))
        } else {
            None
        }
    }
}
