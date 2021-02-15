use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

pub struct EnsuredPool<K: Hash + Eq, V> {
    load_factor: f64,
    items: HashMap<K, V>,
    lru: PriorityQueue<K, Reverse<Instant>>,
}

impl<K: Hash + Eq, V> EnsuredPool<K, V> {}

impl<K: Hash + Eq, V> Default for EnsuredPool<K, V> {
    fn default() -> Self {
        EnsuredPool {
            load_factor: 2.5,
            items: Default::default(),
            lru: Default::default(),
        }
    }
}

impl<K: Hash + Eq + Clone, V> EnsuredPool<K, V> {
    pub fn with_load_factor(self, load_factor: f64)->Self{
        Self{
            load_factor,
            ..self
        }
    }

    pub fn ensure<O>(
        &mut self,
        entries: impl Iterator<Item = O>,
        get_key: impl Fn(&O) -> &K,
        make: impl Fn(O) -> V,
    ) -> bool {
        let now = Reverse(Instant::now());
        let mut added = 0;
        let mut ensured = 0;
        for entry in entries {
            let key = get_key(&entry).clone();
            ensured += 1;
            self.items.entry(key.clone()).or_insert_with_key(|_| {
                added += 1;
                make(entry)
            });
            self.lru.push(key, now );
        }

        let max_items = (self.load_factor * (ensured as f64)).floor() as isize;
        let removed = 0.max((self.items.len() as isize) - max_items);
        for _ in 0..removed {
            if let Some((key, _)) = self.lru.pop() {
                self.items.remove(&key);
            }
        }
        let changed = added > 0 || removed > 0;
        if changed {
            log::info!(
                "Ensuring pool {}t/{}v/+{}/-{}/{}max items",
                self.items.len(),
                ensured,
                added,
                removed,
                max_items
            );
        }

        added > 0 || removed > 0
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.items.get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.items.get_mut(key)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> {
        self.items.iter()
    }

    pub fn entries_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.items.iter_mut()
    }

    pub fn len(&self)->usize{
        self.items.len()
    }
}

#[cfg(test)]
mod test{
    use crate::ensured_pool::EnsuredPool;
    use std::collections::HashSet;
    use itertools::Itertools;

    #[test]
    fn test_ensured_pool(){
        let mut pool: EnsuredPool<usize, String> = EnsuredPool::default().with_load_factor(2.);

        pool.ensure(1..=5, |i|i , |i|i.to_string());

        assert_eq!(5, pool.len());

        assert_eq!(pool.get(&3).expect("present"), &"3".to_string());

        pool.ensure(6..=10, |i|i, |i|i.to_string());

        assert_eq!(10, pool.len());

        let keys: Vec<_> = pool.entries().map(|(k, v)| k).copied().sorted().collect_vec();

        assert_eq!((1..=10).collect_vec(), keys);

        pool.ensure(100..=105, |i|i, |i|i.to_string());

        let keys: Vec<_> = pool.entries().map(|(k, v)| k).copied().sorted().collect_vec();
        assert_eq!(12, keys.len());
        let (chance, retained) = keys.split_at(1);

        assert_eq!((6..=10).chain(100..=105).collect_vec(), retained.to_vec());

        assert!(chance.iter().all(|x|*x<6));
    }
}