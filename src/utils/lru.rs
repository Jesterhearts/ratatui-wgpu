use std::{
    fmt::Debug,
    hash::Hash,
};

use indexmap::IndexMap;

#[derive(Debug)]
struct Entry<Value> {
    age: u64,
    value: Value,
}

pub(crate) struct Lru<Key, Value> {
    queue: IndexMap<Key, Entry<Value>>,
    age: u64,
}

impl<Key, Value> Default for Lru<Key, Value> {
    fn default() -> Self {
        Self {
            queue: Default::default(),
            age: u64::MAX,
        }
    }
}

impl<Key: Hash + Eq, Value> Lru<Key, Value> {
    pub(crate) fn clear(&mut self) {
        self.queue.clear();
        self.age = u64::MAX;
    }

    pub(crate) fn get_or_insert_with(
        &mut self,
        key: Key,
        or_insert: impl FnOnce() -> Value,
    ) -> &Value {
        let index = match self.queue.entry(key) {
            indexmap::map::Entry::Occupied(o) => o.index(),
            indexmap::map::Entry::Vacant(v) => {
                let index = v.index();
                v.insert(Entry {
                    age: self.age,
                    value: or_insert(),
                });
                index
            }
        };

        self.queue[index].age = self.age;
        self.age -= 1;
        let index = self.bubble_down(index);
        &self.queue[index].value
    }

    pub(crate) fn insert(&mut self, key: Key, value: Value) -> &Value {
        if self.age == 0 {
            self.clear();
        }

        let index = self
            .queue
            .insert_full(
                key,
                Entry {
                    age: self.age,
                    value,
                },
            )
            .0;
        self.age -= 1;

        let index = self.bubble_down(index);

        &self.queue[index].value
    }

    pub(crate) fn get(&mut self, key: &Key) -> Option<&Value> {
        if self.age == 0 {
            self.clear();
            return None;
        }

        if let Some(mut index) = self.queue.get_index_of(key) {
            self.queue[index].age = self.age;
            self.age -= 1;

            index = self.bubble_down(index);

            Some(&self.queue[index].value)
        } else {
            None
        }
    }

    pub(crate) fn pop(&mut self) -> Option<(Key, Value)> {
        self.pop_internal().map(|(key, entry)| (key, entry.value))
    }

    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    fn pop_internal(&mut self) -> Option<(Key, Entry<Value>)> {
        if self.queue.is_empty() {
            return None;
        }

        let result = self.queue.swap_remove_index(0);
        self.bubble_down(0);
        result
    }

    fn bubble_down(&mut self, mut index: usize) -> usize {
        loop {
            let left_idx = index * 2 + 1;
            let right_idx = index * 2 + 2;

            if left_idx >= self.queue.len() {
                break;
            }

            if right_idx >= self.queue.len() {
                if self.queue[left_idx].age > self.queue[index].age {
                    self.queue.swap_indices(index, left_idx);
                    index = left_idx;
                }
                break;
            }

            let target = if self.queue[left_idx].age > self.queue[right_idx].age {
                left_idx
            } else {
                right_idx
            };

            if self.queue[target].age < self.queue[index].age {
                break;
            }

            self.queue.swap_indices(index, target);
            index = target;
        }

        index
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.queue.values_mut().map(|v| &mut v.value)
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{
        BuildHasher,
        DefaultHasher,
        Hasher,
        RandomState,
    };

    use crate::utils::lru::Lru;

    #[test]
    fn trivial() {
        let mut lru = Lru::default();

        for key in 0..5 {
            lru.insert(key, key);
        }

        let mut vals = vec![];
        while let Some((_, v)) = lru.pop() {
            vals.push(v);
        }

        // We pushed in ascending order, meaning the oldest value is the smallest one.
        assert_eq!(vals, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn heapify() {
        let mut lru = Lru::default();

        for key in 0..500 {
            lru.insert(key, key);
        }

        let rand = RandomState::new();
        let seed = rand.hash_one(0);
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(seed);

        for key in 0..500 {
            hasher.write_i32(key);
            if hasher.finish() % 2 == 0 {
                lru.get(&key);
            }
        }

        let mut lru_vals = vec![];
        while let Some((_, entry)) = lru.pop_internal() {
            lru_vals.push(entry.age);
        }

        assert_eq!(
            lru_vals
                .windows(2)
                .enumerate()
                .find(|(_, lr)| lr[0] <= lr[1]),
            None,
            "LRU Failed to order ages in descending order using seed {}",
            seed,
        );
    }

    #[test]
    fn heapify_get_or_insert() {
        let mut lru = Lru::default();

        for key in 0..500 {
            lru.insert(key, key);
        }

        let rand = RandomState::new();
        let seed = rand.hash_one(0);
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(seed);

        for key in 0..1000 {
            hasher.write_i32(key);
            if hasher.finish() % 2 == 0 {
                lru.get_or_insert_with(key, || key);
            }
        }

        let mut lru_vals = vec![];
        while let Some((_, entry)) = lru.pop_internal() {
            lru_vals.push(entry.age);
        }

        assert_eq!(
            lru_vals
                .windows(2)
                .enumerate()
                .find(|(_, lr)| lr[0] <= lr[1]),
            None,
            "LRU Failed to order ages in descending order using seed {}",
            seed,
        );
    }
}
