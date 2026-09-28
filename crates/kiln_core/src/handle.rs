//! Generational handles.
//!
//! A [`Handle<T>`] is a small `Copy` id (index + generation) into a [`HandlePool<T>`].
//! When a value is removed its slot's generation is bumped, so stale handles are
//! detected instead of silently aliasing whatever reuses the slot.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Typed, generational reference to a value stored in a [`HandlePool<T>`].
pub struct Handle<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Build a handle from raw parts. Mostly useful for serialization and tests.
    pub const fn from_raw_parts(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    /// Slot index inside the owning pool.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot at the time this handle was created.
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Reinterpret this handle as pointing at a different type.
    pub const fn cast<U>(self) -> Handle<U> {
        Handle::from_raw_parts(self.index, self.generation)
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (u64::from(self.generation) << 32 | u64::from(self.index)).hash(state);
    }
}
impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index, self.generation).cmp(&(other.index, other.generation))
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = std::any::type_name::<T>();
        let short = name.rsplit("::").next().unwrap_or(name);
        write!(f, "Handle<{short}>({}v{})", self.index, self.generation)
    }
}

#[derive(Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Owning storage addressed by [`Handle<T>`]. Insert, remove and lookup are O(1).
///
/// Slots are recycled. When a slot's generation would overflow the slot is retired
/// permanently, so handles are never ambiguous.
#[derive(Debug)]
pub struct HandlePool<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for HandlePool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> HandlePool<T> {
    /// Create an empty pool.
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    /// Number of live values.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` when the pool holds no live values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Store `value`, returning a handle to it.
    ///
    /// # Panics
    /// Panics if more than `u32::MAX` slots would be needed.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return Handle::from_raw_parts(index, slot.generation);
        }
        let index = u32::try_from(self.slots.len()).expect("HandlePool exceeded u32::MAX slots");
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        Handle::from_raw_parts(index, 0)
    }

    /// Remove the value behind `handle`. Returns `None` if the handle is stale.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        let value = slot.value.take()?;
        self.len -= 1;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(handle.index);
        }
        // else: retire the slot forever; it is never pushed to the free list.
        Some(value)
    }

    /// `true` if `handle` refers to a live value.
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Borrow the value behind `handle`, or `None` if it is stale.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_ref()
        } else {
            None
        }
    }

    /// Mutably borrow the value behind `handle`, or `None` if it is stale.
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_mut()
        } else {
            None
        }
    }

    /// Iterate over all live `(handle, value)` pairs in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, slot)| {
            let value = slot.value.as_ref()?;
            Some((Handle::from_raw_parts(i as u32, slot.generation), value))
        })
    }

    /// Iterate mutably over all live `(handle, value)` pairs in slot order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<T>, &mut T)> {
        self.slots.iter_mut().enumerate().filter_map(|(i, slot)| {
            let generation = slot.generation;
            let value = slot.value.as_mut()?;
            Some((Handle::from_raw_parts(i as u32, generation), value))
        })
    }

    /// Remove every value. Outstanding handles become stale.
    pub fn clear(&mut self) {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.value.take().is_some()
                && let Some(next) = slot.generation.checked_add(1)
            {
                slot.generation = next;
                self.free.push(i as u32);
            }
        }
        self.len = 0;
    }

    #[cfg(test)]
    fn force_generation(&mut self, index: u32, generation: u32) {
        self.slots[index as usize].generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    /// TC-CORE-01: remove + reinsert reuses the slot with a bumped generation.
    #[test]
    fn tc_core_01_slot_reuse_bumps_generation() {
        let mut pool = HandlePool::new();
        let a = pool.insert("a");
        assert_eq!(pool.remove(a), Some("a"));
        let b = pool.insert("b");
        assert_eq!(b.index(), a.index(), "slot should be reused");
        assert_eq!(b.generation(), a.generation() + 1);
        assert_eq!(pool.get(a), None, "stale handle must not resolve");
        assert_eq!(pool.get(b), Some(&"b"));
        assert_eq!(
            pool.remove(a),
            None,
            "double remove via stale handle is a no-op"
        );
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn generation_overflow_retires_slot() {
        let mut pool = HandlePool::new();
        let h = pool.insert(1);
        pool.force_generation(h.index(), u32::MAX);
        let h = Handle::from_raw_parts(h.index(), u32::MAX);
        assert_eq!(pool.remove(h), Some(1));
        let next = pool.insert(2);
        assert_ne!(next.index(), h.index(), "retired slot must never be reused");
    }

    #[test]
    fn clear_invalidates_all_handles() {
        let mut pool = HandlePool::new();
        let hs: Vec<_> = (0..10).map(|i| pool.insert(i)).collect();
        pool.clear();
        assert!(pool.is_empty());
        assert!(hs.iter().all(|h| !pool.contains(*h)));
        let h = pool.insert(99);
        assert_eq!(pool.get(h), Some(&99));
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u32),
        Remove(usize),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            any::<u32>().prop_map(Op::Insert),
            any::<usize>().prop_map(Op::Remove)
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// TC-CORE-02: random insert/remove sequences always agree with a HashMap model.
        #[test]
        fn tc_core_02_matches_model(ops in proptest::collection::vec(op(), 0..10_000)) {
            let mut pool = HandlePool::new();
            let mut model: HashMap<Handle<u32>, u32> = HashMap::new();
            let mut ever: Vec<Handle<u32>> = Vec::new();
            for op in ops {
                match op {
                    Op::Insert(v) => {
                        let h = pool.insert(v);
                        prop_assert!(model.insert(h, v).is_none(), "fresh handle collided");
                        ever.push(h);
                    }
                    Op::Remove(i) if !ever.is_empty() => {
                        let h = ever[i % ever.len()];
                        prop_assert_eq!(pool.remove(h), model.remove(&h));
                    }
                    Op::Remove(_) => {}
                }
                prop_assert_eq!(pool.len(), model.len());
            }
            for h in &ever {
                prop_assert_eq!(pool.get(*h), model.get(h));
            }
            prop_assert_eq!(pool.iter().count(), model.len());
        }
    }
}
