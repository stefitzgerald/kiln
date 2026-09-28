use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::entity::Entities;
use crate::query::{
    Access, QueryData, QueryError, QueryFilter, QueryIter, ReadOnlyQueryData, WorldPtr,
};
use crate::storage::{AnyStorage, SparseSet};
use crate::{Bundle, Component, Entity, Resource};

/// Error returned by entity operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EntityError {
    /// The entity was despawned or never existed in this world.
    #[error("entity {0} does not exist")]
    NoSuchEntity(Entity),
}

/// Container for entities, their components and global resources.
#[derive(Default)]
pub struct World {
    entities: Entities,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
    resources: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.entities.len())
            .field("component_types", &self.storages.len())
            .field("resources", &self.resources.len())
            .finish()
    }
}

impl World {
    /// Create an empty world.
    pub fn new() -> Self {
        Self::default()
    }

    // ---- Entities ----------------------------------------------------------------------

    /// Spawn an entity with the given components (a single component or a tuple).
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let e = self.entities.alloc();
        bundle.insert_into(self, e);
        e
    }

    /// Spawn an entity with no components.
    pub fn spawn_empty(&mut self) -> Entity {
        self.entities.alloc()
    }

    /// Despawn `entity`, dropping all its components. Returns `false` if it was not alive.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.free(entity) {
            return false;
        }
        for storage in self.storages.values_mut() {
            storage.remove_entity(entity);
        }
        true
    }

    /// `true` if `entity` is alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Iterate all live entities.
    pub fn iter_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities.iter_alive()
    }

    // ---- Components --------------------------------------------------------------------

    /// Add (or replace) components on an existing entity.
    pub fn insert<B: Bundle>(&mut self, entity: Entity, bundle: B) -> Result<(), EntityError> {
        if !self.is_alive(entity) {
            return Err(EntityError::NoSuchEntity(entity));
        }
        bundle.insert_into(self, entity);
        Ok(())
    }

    /// Insert one component. Caller guarantees `entity` is alive.
    pub(crate) fn insert_one<T: Component>(&mut self, entity: Entity, value: T) {
        debug_assert!(self.is_alive(entity));
        self.storage_mut_or_init::<T>().insert(entity, value);
    }

    /// Remove and return a component.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        if !self.is_alive(entity) {
            return None;
        }
        self.storage_mut::<T>()?.remove(entity)
    }

    /// Borrow a component.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.storage::<T>()?.get(entity)
    }

    /// Mutably borrow a component.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        self.storage_mut::<T>()?.get_mut(entity)
    }

    /// `true` if `entity` has a `T`.
    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.storage::<T>().is_some_and(|s| s.contains(entity))
    }

    pub(crate) fn storage<T: Component>(&self) -> Option<&SparseSet<T>> {
        self.storages
            .get(&TypeId::of::<T>())?
            .as_any()
            .downcast_ref()
    }

    pub(crate) fn storage_mut<T: Component>(&mut self) -> Option<&mut SparseSet<T>> {
        self.storages
            .get_mut(&TypeId::of::<T>())?
            .as_any_mut()
            .downcast_mut()
    }

    fn storage_mut_or_init<T: Component>(&mut self) -> &mut SparseSet<T> {
        self.storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(SparseSet::<T>::default()))
            .as_any_mut()
            .downcast_mut()
            .expect("storage registered under the wrong TypeId")
    }

    // ---- Queries -----------------------------------------------------------------------

    /// Iterate entities matching `D`.
    ///
    /// ```
    /// # use kiln_ecs::*;
    /// # #[derive(Debug)] struct Pos(f32); impl Component for Pos {}
    /// # #[derive(Debug)] struct Vel(f32); impl Component for Vel {}
    /// let mut world = World::new();
    /// world.spawn((Pos(0.0), Vel(1.0)));
    /// for (pos, vel) in world.query::<(&mut Pos, &Vel)>() {
    ///     pos.0 += vel.0;
    /// }
    /// ```
    ///
    /// # Panics
    /// Panics if `D` requests conflicting access, e.g. `(&mut A, &A)`. Use
    /// [`World::try_query`] to handle that as an error.
    pub fn query<D: QueryData>(&mut self) -> QueryIter<'_, D, ()> {
        self.query_filtered::<D, ()>()
    }

    /// Like [`World::query`] with an additional filter such as `With<T>` or `Without<T>`.
    pub fn query_filtered<D: QueryData, F: QueryFilter>(&mut self) -> QueryIter<'_, D, F> {
        match self.try_query_filtered::<D, F>() {
            Ok(q) => q,
            Err(e) => panic!("invalid query `{}`: {e}", std::any::type_name::<D>()),
        }
    }

    /// Fallible [`World::query`].
    pub fn try_query<D: QueryData>(&mut self) -> Result<QueryIter<'_, D, ()>, QueryError> {
        self.try_query_filtered::<D, ()>()
    }

    /// Fallible [`World::query_filtered`].
    pub fn try_query_filtered<D: QueryData, F: QueryFilter>(
        &mut self,
    ) -> Result<QueryIter<'_, D, F>, QueryError> {
        let mut access = Access::default();
        D::access(&mut access)?;
        // SAFETY: we hold `&mut self` for the iterator's lifetime and access was validated.
        Ok(unsafe { QueryIter::new(WorldPtr::from_mut(self)) })
    }

    /// Read-only query through a shared borrow.
    pub fn query_ref<D: ReadOnlyQueryData, F: QueryFilter>(&self) -> QueryIter<'_, D, F> {
        // SAFETY: read-only queries never write through the pointer, so a shared borrow is
        // sufficient; read-only access cannot conflict.
        unsafe { QueryIter::new(WorldPtr::from_ref(self)) }
    }

    /// Fetch `D` for a single entity.
    pub fn query_one<D: QueryData>(&mut self, entity: Entity) -> Option<D::Item<'_>> {
        let mut access = Access::default();
        if let Err(e) = D::access(&mut access) {
            panic!("invalid query `{}`: {e}", std::any::type_name::<D>());
        }
        if !self.is_alive(entity) {
            return None;
        }
        // SAFETY: exclusive borrow held for the item's lifetime; access validated above.
        unsafe {
            let fetch = D::init(WorldPtr::from_mut(self))?;
            D::get(fetch, entity)
        }
    }

    pub(crate) fn alive_entities(&self) -> Vec<Entity> {
        self.entities.iter_alive().collect()
    }

    // ---- Resources ---------------------------------------------------------------------

    /// Insert a resource, returning the previous value of that type.
    pub fn insert_resource<R: Resource>(&mut self, value: R) -> Option<R> {
        self.resources
            .insert(TypeId::of::<R>(), Box::new(value))
            .and_then(|old| old.downcast().ok().map(|b: Box<R>| *b))
    }

    /// Insert `R::default()` if absent and return it.
    pub fn init_resource<R: Resource + Default>(&mut self) -> &mut R {
        self.resources
            .entry(TypeId::of::<R>())
            .or_insert_with(|| Box::new(R::default()))
            .downcast_mut()
            .expect("resource registered under the wrong TypeId")
    }

    /// Remove and return a resource.
    pub fn remove_resource<R: Resource>(&mut self) -> Option<R> {
        self.resources
            .remove(&TypeId::of::<R>())?
            .downcast()
            .ok()
            .map(|b: Box<R>| *b)
    }

    /// Borrow a resource.
    pub fn resource<R: Resource>(&self) -> Option<&R> {
        self.resources.get(&TypeId::of::<R>())?.downcast_ref()
    }

    /// Mutably borrow a resource.
    pub fn resource_mut<R: Resource>(&mut self) -> Option<&mut R> {
        self.resources.get_mut(&TypeId::of::<R>())?.downcast_mut()
    }

    /// `true` if a resource of type `R` exists.
    pub fn contains_resource<R: Resource>(&self) -> bool {
        self.resources.contains_key(&TypeId::of::<R>())
    }

    /// Temporarily take resource `R` out of the world so `f` can use it alongside
    /// `&mut World`. Returns `None` if the resource does not exist.
    pub fn resource_scope<R: Resource, U>(
        &mut self,
        f: impl FnOnce(&mut World, &mut R) -> U,
    ) -> Option<U> {
        let mut r = self.remove_resource::<R>()?;
        let out = f(self, &mut r);
        self.insert_resource(r);
        Some(out)
    }
}
