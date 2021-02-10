use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

pub struct EnsuredPool<K: Hash + Eq, V> {
    items: HashMap<K, V>,
    lru: PriorityQueue<K, Reverse<Instant>>,
}

impl<K: Hash + Eq, V> EnsuredPool<K, V> {}

impl<K: Hash + Eq, V> Default for EnsuredPool<K, V> {
    fn default() -> Self {
        EnsuredPool {
            items: Default::default(),
            lru: Default::default(),
        }
    }
}

impl<K: Hash + Eq + Clone, V> EnsuredPool<K, V> {
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
            self.items.entry(key.clone()).or_insert_with_key(|key| {
                added += 1;
                make(entry)
            });
            self.lru.push(key, now);
        }

        let max_items = (2.5 * (ensured as f64)).floor() as isize;
        let removed = 0.max((self.items.len() as isize) - max_items);
        for _ in 0..removed {
            if let Some((key, _)) = self.lru.pop() {
                self.items.remove(&key);
            }
        }
        let changed = added > 0 || removed > 0;
        if changed {
            log::info!(
                "Ensuring pool {}t/{}v/+{}/-{} items",
                self.items.len(),
                ensured,
                added,
                removed
            );
        }

        added > 0 || removed > 0
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
}
