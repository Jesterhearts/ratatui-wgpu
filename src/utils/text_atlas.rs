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
use ratatui::style::Modifier;

use crate::utils::lru::Lru;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Key {
    pub(crate) style: Modifier,
    pub(crate) glyph: u32,
    pub(crate) font: u64,
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

#[derive(Debug)]
struct CacheEntry {
    id: AllocId,
    value: CacheRect,
}

pub(crate) struct Atlas {
    lru: Lru<Key, CacheEntry>,
    inner: AtlasAllocator,
}

impl Atlas {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Atlas {
            lru: Lru::default(),
            inner: AtlasAllocator::new(Size::new(width as i32, height as i32)),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.lru.clear();
        self.inner.clear();
    }

    pub(crate) fn try_get(&mut self, key: &Key) -> Option<Entry> {
        self.lru.get(key).map(|e| Entry::Cached(e.value))
    }

    pub(crate) fn get(&mut self, key: &Key, width: u32, height: u32) -> Entry {
        self.try_get(key).unwrap_or_else(|| {
            let mut tries = 1;
            loop {
                if let Some(alloc) = self.inner.allocate(Size::new(width as i32, height as i32)) {
                    let rect = alloc.rectangle.into();
                    self.lru.insert(
                        *key,
                        CacheEntry {
                            id: alloc.id,
                            value: alloc.rectangle.into(),
                        },
                    );

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

                    for rect in self.lru.iter_mut() {
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
