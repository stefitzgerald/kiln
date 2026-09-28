use std::any::Any;

use crate::{Component, Entity};

const EMPTY: u32 = u32::MAX;

/// Sparse-set component storage: O(1) insert/remove/lookup, densely packed iteration.
#[derive(Debug)]
#[allow(unreachable_pub)] // Appears in `QueryData::Fetch` types but lives in a private module.
pub struct SparseSet<T> {
    /// Entity index → position in `dense`/`data`, or [`EMPTY`].
    sparse: Vec<u32>,
    dense: Vec<Entity>,
    data: Vec<T>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            data: Vec::new(),
        }
    }
}

impl<T> SparseSet<T> {
    fn slot(&self, entity: Entity) -> Option<usize> {
        let d = *self.sparse.get(entity.index() as usize)?;
        (d != EMPTY && self.dense[d as usize] == entity).then_some(d as usize)
    }

    /// Insert or replace. Returns the previous value.
    pub(crate) fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        if let Some(d) = self.slot(entity) {
            return Some(std::mem::replace(&mut self.data[d], value));
        }
        let i = entity.index() as usize;
        if self.sparse.len() <= i {
            self.sparse.resize(i + 1, EMPTY);
        }
        // A stale entity with the same index may still occupy the slot; that cannot happen
        // because the world removes components on despawn, but guard anyway.
        debug_assert_eq!(self.sparse[i], EMPTY, "stale component left behind");
        self.sparse[i] = u32::try_from(self.dense.len()).expect("component storage overflow");
        self.dense.push(entity);
        self.data.push(value);
        None
    }

    pub(crate) fn remove(&mut self, entity: Entity) -> Option<T> {
        let d = self.slot(entity)?;
        self.sparse[entity.index() as usize] = EMPTY;
        self.dense.swap_remove(d);
        let value = self.data.swap_remove(d);
        if let Some(moved) = self.dense.get(d) {
            self.sparse[moved.index() as usize] = d as u32;
        }
        Some(value)
    }

    pub(crate) fn get(&self, entity: Entity) -> Option<&T> {
        self.slot(entity).map(|d| &self.data[d])
    }

    pub(crate) fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        self.slot(entity).map(|d| &mut self.data[d])
    }

    pub(crate) fn contains(&self, entity: Entity) -> bool {
        self.slot(entity).is_some()
    }

    // ---- Raw access used by queries -------------------------------------------------
    //
    // Queries hold raw pointers to several storages at once. These helpers only ever create
    // references to individual *fields* (`sparse`, `dense`, one element of `data`), never to
    // the whole `SparseSet`, so shared reads of `sparse`/`dense` coexist with exclusive
    // access to disjoint elements of `data`.

    /// # Safety
    /// `this` must point to a live `SparseSet<T>` whose `sparse` and `dense` fields are not
    /// being mutated for the duration of the call.
    pub(crate) unsafe fn raw_slot(this: *const Self, entity: Entity) -> Option<usize> {
        // SAFETY: guaranteed by the caller.
        let (sparse, dense) = unsafe { (&(*this).sparse, &(*this).dense) };
        let d = *sparse.get(entity.index() as usize)?;
        (d != EMPTY && dense[d as usize] == entity).then_some(d as usize)
    }

    /// # Safety
    /// As [`SparseSet::raw_slot`], and `dense` must not be mutated during `'a`.
    pub(crate) unsafe fn raw_entities<'a>(this: *const Self) -> &'a [Entity] {
        // SAFETY: guaranteed by the caller.
        unsafe { (*this).dense.as_slice() }
    }

    /// # Safety
    /// As [`SparseSet::raw_slot`], and no exclusive reference to the returned element may
    /// exist during `'a`.
    pub(crate) unsafe fn raw_get<'a>(this: *const Self, entity: Entity) -> Option<&'a T> {
        // SAFETY: guaranteed by the caller; `d` is in bounds of `data`.
        unsafe {
            let d = Self::raw_slot(this, entity)?;
            Some(&*(*this).data.as_ptr().add(d))
        }
    }

    /// # Safety
    /// As [`SparseSet::raw_slot`], `this` must be derived from an exclusive borrow, and no
    /// other reference to the returned element may exist during `'a`.
    pub(crate) unsafe fn raw_get_mut<'a>(this: *mut Self, entity: Entity) -> Option<&'a mut T> {
        // SAFETY: guaranteed by the caller; `d` is in bounds of `data`.
        unsafe {
            let d = Self::raw_slot(this, entity)?;
            let base = std::ptr::addr_of_mut!((*this).data);
            Some(&mut *(*base).as_mut_ptr().add(d))
        }
    }
}

/// Type-erased component storage.
pub(crate) trait AnyStorage: Send + Sync {
    fn remove_entity(&mut self, entity: Entity) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Component> AnyStorage for SparseSet<T> {
    fn remove_entity(&mut self, entity: Entity) -> bool {
        self.remove(entity).is_some()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_remove_keeps_sparse_consistent() {
        let mut s = SparseSet::default();
        let es: Vec<_> = (0..5).map(|i| Entity::from_raw_parts(i, 0)).collect();
        for (i, e) in es.iter().enumerate() {
            s.insert(*e, i);
        }
        assert_eq!(s.remove(es[1]), Some(1));
        assert_eq!(s.remove(es[1]), None);
        for (i, e) in es.iter().enumerate().filter(|(i, _)| *i != 1) {
            assert_eq!(s.get(*e), Some(&i));
        }
        assert_eq!(s.insert(es[0], 42), Some(0));
        // Different generation must not match.
        assert!(!s.contains(Entity::from_raw_parts(0, 1)));
    }
}
