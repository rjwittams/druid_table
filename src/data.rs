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
        (0usize..self.len()).map(|i| {
            self.use_item(i, |item| {
                item.clone()
            })
        }).flatten().collect()
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

pub struct RemappedItems<U: ItemsUse> {
    underlying: U,
    remap: Option<Vec<usize>>, // TODO : Adaptive storage for
                               // the case where only a few remappings have been done, ie manual moves.
                               // Runs of shifts in a Vec<(usize,usize)>
                               // Sorting and filtering will always provide a full vec
}

impl<U: ItemsUse> RemappedItems<U> {
    pub fn new(underlying: U, remap: Option<Vec<usize>>) -> Self {
        RemappedItems { underlying, remap }
    }
}

impl<U: ItemsUse + Data> ItemsLen for RemappedItems<U> {
    fn len(&self) -> usize {
        if let Some(remap) = &self.remap {
            remap.len()
        } else {
            self.underlying.len()
        }
    }
}

impl<U: ItemsUse + Data> ItemsUse for RemappedItems<U> {
    type Item = U::Item;

    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V> {
        if let Some(remap) = &self.remap {
            remap
                .get(idx)
                .and_then(|new_idx| self.underlying.use_item(*new_idx, f))
        } else {
            self.underlying.use_item(idx, f)
        }
    }
}

#[cfg(test)]
mod test {
    use crate::data::*;
    use im::Vector;

    #[test]
    fn remap() {
        let und: Vector<usize> = (0usize..=10).into_iter().collect();
        assert_eq!(und, und.all_items());
        let mut remapped = RemappedItems::new(und.clone(), None);
        assert_eq!(Some(0), remapped.use_item(0, |u|*u));
        assert_eq!(Some(7), remapped.use_item(7, |u|*u));
        assert_eq!(und, remapped.all_items());
        let remap_idxs = vec![8, 7, 5, 1];
        remapped.remap = Some(remap_idxs.clone());

        let res: Vector<usize> = remap_idxs.into_iter().collect();
        assert_eq!(res, remapped.all_items());
    }

}
