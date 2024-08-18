use rustybuzz::{
    Direction,
    Script,
    ShapePlan,
    UnicodeBuffer,
};

use crate::{
    utils::lru::Lru,
    Font,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    face_id: u64,
    direction: Direction,
    script: Script,
}

pub(crate) struct PlanCache {
    lru: Lru<Key, ShapePlan>,
    capacity: usize,
}

impl PlanCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            lru: Lru::default(),
            capacity: capacity + 1,
        }
    }

    pub(crate) fn get(&mut self, font: &Font, buffer: &mut UnicodeBuffer) -> &ShapePlan {
        buffer.guess_segment_properties();
        let key = Key {
            face_id: font.id(),
            direction: buffer.direction(),
            script: buffer.script(),
        };

        if self.lru.len() == self.capacity {
            self.lru.pop();
        }

        self.lru.get_or_insert_with(key, || {
            ShapePlan::new(
                font.font(),
                buffer.direction(),
                Some(buffer.script()),
                buffer.language().as_ref(),
                &[],
            )
        })
    }
}
