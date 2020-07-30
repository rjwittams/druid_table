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
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V>;
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
        self.get(idx).map(move |x| f(x))
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
