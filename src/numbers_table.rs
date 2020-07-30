use crate::{ItemsLen, ItemsUse};
use druid::{Data, Lens};

#[derive(Debug, Data, Clone, Lens)]
pub struct NumbersTable {
    rows: usize,
}

impl NumbersTable {
    pub fn new(rows: usize) -> Self {
        NumbersTable { rows }
    }
}

impl ItemsLen for NumbersTable {
    fn len(&self) -> usize {
        self.rows
    }
}

impl ItemsUse for NumbersTable {
    type Item = usize;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&usize) -> V) -> Option<V> {
        if idx < self.rows {
            Some(f(&idx))
        } else {
            None
        }
    }
}
