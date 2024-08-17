use std::{
    collections::HashMap,
    ops::Deref,
};

use guillotiere::{
    AllocId,
    AtlasAllocator,
    Change,
    Rectangle,
    Size,
};
use priority_queue::PriorityQueue;
use ratatui::style::Modifier;
use swash::CacheKey;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Key {
    pub(crate) style: Modifier,
    pub(crate) glyph: u16,
    pub(crate) font: CacheKey,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct CacheRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl From<Rectangle> for CacheRect {
    fn from(value: Rectangle) -> Self {
        Self {
            x: value.min.x as u32,
            y: value.min.y as u32,
            width: value.width() as u32,
            height: value.height() as u32,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Entry {
    Cached(CacheRect),
    Uncached(CacheRect),
}

impl Entry {
    pub(crate) fn cached(&self) -> bool {
        matches!(self, Entry::Cached(_))
    }
}

impl Deref for Entry {
    type Target = CacheRect;

    fn deref(&self) -> &Self::Target {
        let (Entry::Cached(entry) | Entry::Uncached(entry)) = self;
        entry
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CacheEntry {
    age: u64,
    id: AllocId,
    value: CacheRect,
}

impl PartialOrd for CacheEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CacheEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.age.cmp(&other.age)
    }
}

pub(crate) struct Atlas {
    lru: PriorityQueue<Key, CacheEntry>,
    inner: AtlasAllocator,

    next_age: u64,
}

impl Atlas {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Atlas {
            lru: PriorityQueue::new(),
            inner: AtlasAllocator::new(Size::new(width as i32, height as i32)),
            next_age: u64::MAX,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.lru.clear();
        self.inner.clear();
        self.next_age = u64::MAX;
    }

    pub(crate) fn try_get(&mut self, key: &Key) -> Option<Entry> {
        if self.next_age == 0 {
            self.clear();
            return None;
        }

        let mut existing = None;
        self.lru.change_priority_by(key, |entry| {
            entry.age = self.next_age;
            existing = Some(entry.value);
            self.next_age -= 1;
        });
        existing.map(Entry::Cached)
    }

    pub(crate) fn get(&mut self, key: &Key, width: u32, height: u32) -> Entry {
        self.try_get(key).unwrap_or_else(|| {
            let mut tries = 1;
            loop {
                if let Some(alloc) = self.inner.allocate(Size::new(width as i32, height as i32)) {
                    let rect = alloc.rectangle.into();
                    self.lru.push(
                        *key,
                        CacheEntry {
                            age: self.next_age,
                            id: alloc.id,
                            value: alloc.rectangle.into(),
                        },
                    );

                    self.next_age -= 1;

                    return Entry::Uncached(rect);
                }

                self.inner.deallocate(self.lru.pop().unwrap().1.id);
                if tries % 10 == 0 {
                    let changes = self
                        .inner
                        .rearrange()
                        .changes
                        .into_iter()
                        .map(|Change { old, new }| (old.id, new))
                        .collect::<HashMap<_, _>>();

                    for (_, rect) in self.lru.iter_mut() {
                        if let Some(new_alloc) = changes.get(&rect.id) {
                            rect.id = new_alloc.id;
                            rect.value = new_alloc.rectangle.into();
                        }
                    }
                }
                tries += 1;
            }
        })
    }
}
