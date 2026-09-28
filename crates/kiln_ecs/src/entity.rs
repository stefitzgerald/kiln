use std::fmt;

/// Lightweight, generational id of an entity in a [`World`](crate::World).
///
/// A despawned entity's index is recycled with a new generation, so old ids never alias
/// newly spawned entities.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// Build an entity id from raw parts. The id is not necessarily alive in any world.
    pub const fn from_raw_parts(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Slot index; unique among live entities.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation; incremented every time the slot is reused.
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Pack into a single `u64` (generation in the high bits).
    pub const fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index, self.generation)
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Entity id allocator.
#[derive(Debug, Default)]
pub(crate) struct Entities {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
    len: usize,
}

impl Entities {
    pub(crate) fn alloc(&mut self) -> Entity {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            self.alive[index as usize] = true;
            return Entity::from_raw_parts(index, self.generations[index as usize]);
        }
        let index = u32::try_from(self.generations.len()).expect("entity index space exhausted");
        self.generations.push(0);
        self.alive.push(true);
        Entity::from_raw_parts(index, 0)
    }

    /// Returns `false` if `entity` was not alive.
    pub(crate) fn free(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }
        let i = entity.index as usize;
        self.alive[i] = false;
        self.len -= 1;
        if let Some(next) = self.generations[i].checked_add(1) {
            self.generations[i] = next;
            self.free.push(entity.index);
        }
        true
    }

    pub(crate) fn is_alive(&self, entity: Entity) -> bool {
        let i = entity.index as usize;
        self.alive.get(i).copied().unwrap_or(false) && self.generations[i] == entity.generation
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn iter_alive(&self) -> impl Iterator<Item = Entity> + '_ {
        self.alive
            .iter()
            .enumerate()
            .filter(|(_, alive)| **alive)
            .map(|(i, _)| Entity::from_raw_parts(i as u32, self.generations[i]))
    }
}
