//! Typed queries over component storages.
//!
//! A query is described by a [`QueryData`] type (what to fetch) and a [`QueryFilter`]
//! (which entities qualify):
//!
//! | `QueryData`        | Item               |
//! |--------------------|--------------------|
//! | `&T`               | `&T`               |
//! | `&mut T`           | `&mut T`           |
//! | `Option<&T>`       | `Option<&T>`       |
//! | `Option<&mut T>`   | `Option<&mut T>`   |
//! | `Entity`           | `Entity`           |
//! | `Has<T>`           | `bool`             |
//! | tuples of the above (up to 8) | tuple   |
//!
//! Filters: `()`, [`With<T>`], [`Without<T>`] and tuples of filters (all must match).
//!
//! Aliasing is checked when the query is created: `(&mut A, &A)` or `(&mut A, &mut A)`
//! returns [`QueryError::ConflictingAccess`].

use std::any::TypeId;
use std::marker::PhantomData;

use crate::storage::SparseSet;
use crate::{Component, Entity, World};

/// Error produced when building a query.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// The same component is accessed mutably more than once, or both mutably and immutably.
    #[error(
        "conflicting access to component `{component}`: a component may be borrowed mutably \
         at most once and not at the same time as immutably"
    )]
    ConflictingAccess {
        /// Type name of the component.
        component: &'static str,
    },
}

/// Set of component types a query reads and writes.
#[derive(Debug, Default)]
pub struct Access {
    reads: Vec<TypeId>,
    writes: Vec<TypeId>,
}

impl Access {
    /// Register a shared borrow of `T`.
    pub fn add_read<T: 'static>(&mut self) -> Result<(), QueryError> {
        let id = TypeId::of::<T>();
        if self.writes.contains(&id) {
            return Err(QueryError::ConflictingAccess {
                component: std::any::type_name::<T>(),
            });
        }
        self.reads.push(id);
        Ok(())
    }

    /// Register an exclusive borrow of `T`.
    pub fn add_write<T: 'static>(&mut self) -> Result<(), QueryError> {
        let id = TypeId::of::<T>();
        if self.writes.contains(&id) || self.reads.contains(&id) {
            return Err(QueryError::ConflictingAccess {
                component: std::any::type_name::<T>(),
            });
        }
        self.writes.push(id);
        Ok(())
    }
}

/// Raw world pointer handed to query fetches.
#[derive(Clone, Copy, Debug)]
pub struct WorldPtr<'w> {
    ptr: *mut World,
    _marker: PhantomData<&'w World>,
}

impl<'w> WorldPtr<'w> {
    pub(crate) fn from_mut(world: &'w mut World) -> Self {
        Self {
            ptr: world,
            _marker: PhantomData,
        }
    }

    pub(crate) fn from_ref(world: &'w World) -> Self {
        Self {
            ptr: world as *const World as *mut World,
            _marker: PhantomData,
        }
    }

    /// # Safety
    /// No exclusive reference to the world may be live.
    unsafe fn world(self) -> &'w World {
        // SAFETY: guaranteed by the caller.
        unsafe { &*self.ptr }
    }

    /// # Safety
    /// Must originate from [`WorldPtr::from_mut`], and the returned reference must be
    /// used only to obtain raw storage pointers.
    unsafe fn world_mut(self) -> &'w mut World {
        // SAFETY: guaranteed by the caller.
        unsafe { &mut *self.ptr }
    }

    fn storage<T: Component>(self) -> Option<*const SparseSet<T>> {
        // SAFETY: only a short-lived shared borrow used to locate the storage.
        unsafe { self.world() }
            .storage::<T>()
            .map(|s| s as *const _)
    }

    /// # Safety
    /// See [`WorldPtr::world_mut`].
    unsafe fn storage_mut<T: Component>(self) -> Option<*mut SparseSet<T>> {
        // SAFETY: guaranteed by the caller.
        unsafe { self.world_mut() }
            .storage_mut::<T>()
            .map(|s| s as *mut _)
    }
}

/// Something a query can fetch per entity.
///
/// # Safety
/// `access` must register every component that `get` reads (as read) or writes (as write);
/// the query machinery relies on it to hand out non-aliasing references.
pub unsafe trait QueryData {
    /// Per-entity result.
    type Item<'w>;
    /// Cached pointers, created once per query.
    type Fetch: Copy;

    /// Register accessed component types.
    fn access(access: &mut Access) -> Result<(), QueryError>;

    /// Prepare to fetch. `None` means no entity can match (e.g. a required storage is absent).
    ///
    /// # Safety
    /// If `Self` writes, `world` must come from an exclusive borrow.
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch>;

    /// Entities that are guaranteed to be a superset of the matches, if this fetch knows them.
    ///
    /// # Safety
    /// `fetch` must be valid for `'w` and the storage's entity list must not change during `'w`.
    unsafe fn candidates<'w>(fetch: Self::Fetch) -> Option<&'w [Entity]>;

    /// Fetch the item for `entity`, or `None` if it does not match.
    ///
    /// # Safety
    /// `fetch` must be valid for `'w`, and no other live item may alias the same entity's
    /// mutably accessed components.
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<Self::Item<'w>>;
}

/// Marker for [`QueryData`] that only reads.
///
/// # Safety
/// Implementors must never write through the fetched pointers.
pub unsafe trait ReadOnlyQueryData: QueryData {}

/// Entity filter for queries.
///
/// # Safety
/// `matches` may only read storage membership.
pub unsafe trait QueryFilter {
    /// Cached pointers.
    type Fetch: Copy;
    /// Prepare the filter.
    fn init(world: WorldPtr<'_>) -> Self::Fetch;
    /// `true` if `entity` passes.
    ///
    /// # Safety
    /// `fetch` must still be valid.
    unsafe fn matches(fetch: Self::Fetch, entity: Entity) -> bool;
}

// ---- QueryData impls -------------------------------------------------------------------

// SAFETY: registers a read of T and only reads T.
unsafe impl<T: Component> QueryData for &T {
    type Item<'w> = &'w T;
    type Fetch = *const SparseSet<T>;

    fn access(access: &mut Access) -> Result<(), QueryError> {
        access.add_read::<T>()
    }
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
        world.storage::<T>()
    }
    unsafe fn candidates<'w>(fetch: Self::Fetch) -> Option<&'w [Entity]> {
        // SAFETY: forwarded contract.
        Some(unsafe { SparseSet::raw_entities(fetch) })
    }
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<&'w T> {
        // SAFETY: forwarded contract; no writer of T exists (access check).
        unsafe { SparseSet::raw_get(fetch, entity) }
    }
}
// SAFETY: only reads.
unsafe impl<T: Component> ReadOnlyQueryData for &T {}

// SAFETY: registers a write of T.
unsafe impl<T: Component> QueryData for &mut T {
    type Item<'w> = &'w mut T;
    type Fetch = *mut SparseSet<T>;

    fn access(access: &mut Access) -> Result<(), QueryError> {
        access.add_write::<T>()
    }
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
        // SAFETY: forwarded contract (exclusive world borrow for writers).
        unsafe { world.storage_mut::<T>() }
    }
    unsafe fn candidates<'w>(fetch: Self::Fetch) -> Option<&'w [Entity]> {
        // SAFETY: forwarded contract.
        Some(unsafe { SparseSet::raw_entities(fetch) })
    }
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<&'w mut T> {
        // SAFETY: forwarded contract; each entity is visited once and T has a single writer.
        unsafe { SparseSet::raw_get_mut(fetch, entity) }
    }
}

// SAFETY: registers a read of T.
unsafe impl<T: Component> QueryData for Option<&T> {
    type Item<'w> = Option<&'w T>;
    type Fetch = Option<*const SparseSet<T>>;

    fn access(access: &mut Access) -> Result<(), QueryError> {
        access.add_read::<T>()
    }
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
        Some(world.storage::<T>())
    }
    unsafe fn candidates<'w>(_: Self::Fetch) -> Option<&'w [Entity]> {
        None
    }
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<Option<&'w T>> {
        // SAFETY: forwarded contract.
        Some(fetch.and_then(|s| unsafe { SparseSet::raw_get(s, entity) }))
    }
}
// SAFETY: only reads.
unsafe impl<T: Component> ReadOnlyQueryData for Option<&T> {}

// SAFETY: registers a write of T.
unsafe impl<T: Component> QueryData for Option<&mut T> {
    type Item<'w> = Option<&'w mut T>;
    type Fetch = Option<*mut SparseSet<T>>;

    fn access(access: &mut Access) -> Result<(), QueryError> {
        access.add_write::<T>()
    }
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
        // SAFETY: forwarded contract.
        Some(unsafe { world.storage_mut::<T>() })
    }
    unsafe fn candidates<'w>(_: Self::Fetch) -> Option<&'w [Entity]> {
        None
    }
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<Option<&'w mut T>> {
        // SAFETY: forwarded contract.
        Some(fetch.and_then(|s| unsafe { SparseSet::raw_get_mut(s, entity) }))
    }
}

// SAFETY: accesses no components.
unsafe impl QueryData for Entity {
    type Item<'w> = Entity;
    type Fetch = ();

    fn access(_: &mut Access) -> Result<(), QueryError> {
        Ok(())
    }
    unsafe fn init(_: WorldPtr<'_>) -> Option<()> {
        Some(())
    }
    unsafe fn candidates<'w>(_: ()) -> Option<&'w [Entity]> {
        None
    }
    unsafe fn get<'w>(_: (), entity: Entity) -> Option<Self::Item<'w>> {
        Some(entity)
    }
}
// SAFETY: only reads.
unsafe impl ReadOnlyQueryData for Entity {}

/// Query item that is `true` when the entity has a `T` (without borrowing it).
#[derive(Debug)]
pub struct Has<T>(PhantomData<T>);

// SAFETY: only reads storage membership, which never aliases component data.
unsafe impl<T: Component> QueryData for Has<T> {
    type Item<'w> = bool;
    type Fetch = Option<*const SparseSet<T>>;

    fn access(_: &mut Access) -> Result<(), QueryError> {
        Ok(())
    }
    unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
        Some(world.storage::<T>())
    }
    unsafe fn candidates<'w>(_: Self::Fetch) -> Option<&'w [Entity]> {
        None
    }
    unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<Self::Item<'w>> {
        // SAFETY: forwarded contract.
        Some(fetch.is_some_and(|s| unsafe { SparseSet::raw_slot(s, entity) }.is_some()))
    }
}
// SAFETY: only reads.
unsafe impl<T: Component> ReadOnlyQueryData for Has<T> {}

macro_rules! impl_query_data_tuple {
    ($($name:ident),+) => {
        // SAFETY: forwards to each element; `access` registers every element's access.
        unsafe impl<$($name: QueryData),+> QueryData for ($($name,)+) {
            type Item<'w> = ($($name::Item<'w>,)+);
            type Fetch = ($($name::Fetch,)+);

            fn access(access: &mut Access) -> Result<(), QueryError> {
                $($name::access(access)?;)+
                Ok(())
            }
            unsafe fn init(world: WorldPtr<'_>) -> Option<Self::Fetch> {
                // SAFETY: forwarded contract.
                unsafe { Some(($($name::init(world)?,)+)) }
            }
            #[allow(non_snake_case)]
            unsafe fn candidates<'w>(fetch: Self::Fetch) -> Option<&'w [Entity]> {
                let ($($name,)+) = fetch;
                let mut best: Option<&'w [Entity]> = None;
                $(
                    // SAFETY: forwarded contract.
                    if let Some(c) = unsafe { $name::candidates($name) } {
                        if best.is_none_or(|b| c.len() < b.len()) {
                            best = Some(c);
                        }
                    }
                )+
                best
            }
            #[allow(non_snake_case)]
            unsafe fn get<'w>(fetch: Self::Fetch, entity: Entity) -> Option<Self::Item<'w>> {
                let ($($name,)+) = fetch;
                // SAFETY: forwarded contract.
                unsafe { Some(($($name::get($name, entity)?,)+)) }
            }
        }
        // SAFETY: every element is read-only.
        unsafe impl<$($name: ReadOnlyQueryData),+> ReadOnlyQueryData for ($($name,)+) {}
    };
}

impl_query_data_tuple!(A);
impl_query_data_tuple!(A, B);
impl_query_data_tuple!(A, B, C);
impl_query_data_tuple!(A, B, C, D);
impl_query_data_tuple!(A, B, C, D, E);
impl_query_data_tuple!(A, B, C, D, E, F);
impl_query_data_tuple!(A, B, C, D, E, F, G);
impl_query_data_tuple!(A, B, C, D, E, F, G, H);

// ---- Filters ---------------------------------------------------------------------------

/// Filter: entity has a `T`.
#[derive(Debug)]
pub struct With<T>(PhantomData<T>);

/// Filter: entity does not have a `T`.
#[derive(Debug)]
pub struct Without<T>(PhantomData<T>);

// SAFETY: always matches; reads nothing.
unsafe impl QueryFilter for () {
    type Fetch = ();
    fn init(_: WorldPtr<'_>) {}
    unsafe fn matches(_: (), _: Entity) -> bool {
        true
    }
}

// SAFETY: reads only membership.
unsafe impl<T: Component> QueryFilter for With<T> {
    type Fetch = Option<*const SparseSet<T>>;
    fn init(world: WorldPtr<'_>) -> Self::Fetch {
        world.storage::<T>()
    }
    unsafe fn matches(fetch: Self::Fetch, entity: Entity) -> bool {
        // SAFETY: forwarded contract.
        fetch.is_some_and(|s| unsafe { SparseSet::raw_slot(s, entity) }.is_some())
    }
}

// SAFETY: reads only membership.
unsafe impl<T: Component> QueryFilter for Without<T> {
    type Fetch = Option<*const SparseSet<T>>;
    fn init(world: WorldPtr<'_>) -> Self::Fetch {
        world.storage::<T>()
    }
    unsafe fn matches(fetch: Self::Fetch, entity: Entity) -> bool {
        // SAFETY: forwarded contract.
        !fetch.is_some_and(|s| unsafe { SparseSet::raw_slot(s, entity) }.is_some())
    }
}

macro_rules! impl_query_filter_tuple {
    ($($name:ident),+) => {
        // SAFETY: forwards to each element.
        unsafe impl<$($name: QueryFilter),+> QueryFilter for ($($name,)+) {
            type Fetch = ($($name::Fetch,)+);
            fn init(world: WorldPtr<'_>) -> Self::Fetch {
                ($($name::init(world),)+)
            }
            #[allow(non_snake_case)]
            unsafe fn matches(fetch: Self::Fetch, entity: Entity) -> bool {
                let ($($name,)+) = fetch;
                // SAFETY: forwarded contract.
                unsafe { true $(&& $name::matches($name, entity))+ }
            }
        }
    };
}

impl_query_filter_tuple!(A);
impl_query_filter_tuple!(A, B);
impl_query_filter_tuple!(A, B, C);
impl_query_filter_tuple!(A, B, C, D);

// ---- Iterator --------------------------------------------------------------------------

enum Candidates<'w> {
    Borrowed(&'w [Entity]),
    Owned(Vec<Entity>),
}

impl Candidates<'_> {
    fn get(&self, i: usize) -> Option<Entity> {
        match self {
            Candidates::Borrowed(s) => s.get(i).copied(),
            Candidates::Owned(v) => v.get(i).copied(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Candidates::Borrowed(s) => s.len(),
            Candidates::Owned(v) => v.len(),
        }
    }
}

/// Iterator over query results. Created by [`World::query`] and friends.
pub struct QueryIter<'w, D: QueryData, F: QueryFilter> {
    fetch: Option<D::Fetch>,
    filter: F::Fetch,
    candidates: Candidates<'w>,
    pos: usize,
    _marker: PhantomData<&'w mut World>,
}

impl<D: QueryData, F: QueryFilter> std::fmt::Debug for QueryIter<'_, D, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryIter")
            .field("query", &std::any::type_name::<D>())
            .field("pos", &self.pos)
            .field("candidates", &self.candidates.len())
            .finish()
    }
}

impl<'w, D: QueryData, F: QueryFilter> QueryIter<'w, D, F> {
    /// # Safety
    /// If `D` writes, `world` must come from an exclusive borrow, and `D`'s access must have
    /// been validated as conflict-free. The world must not be modified during `'w`.
    pub(crate) unsafe fn new(world: WorldPtr<'w>) -> Self {
        let filter = F::init(world);
        // SAFETY: forwarded contract.
        let fetch = unsafe { D::init(world) };
        let candidates = match fetch {
            // SAFETY: storages outlive 'w and are not structurally modified during it.
            Some(f) => match unsafe { D::candidates(f) } {
                Some(slice) => Candidates::Borrowed(slice),
                // SAFETY: short-lived shared read of the entity list.
                None => Candidates::Owned(unsafe { world.world() }.alive_entities()),
            },
            None => Candidates::Borrowed(&[]),
        };
        Self {
            fetch,
            filter,
            candidates,
            pos: 0,
            _marker: PhantomData,
        }
    }
}

impl<'w, D: QueryData, F: QueryFilter> Iterator for QueryIter<'w, D, F> {
    type Item = D::Item<'w>;

    fn next(&mut self) -> Option<Self::Item> {
        let fetch = self.fetch?;
        while let Some(entity) = self.candidates.get(self.pos) {
            self.pos += 1;
            // SAFETY: candidates are unique entities, so every mutable item is handed out at
            // most once; storages stay valid for 'w.
            unsafe {
                if F::matches(self.filter, entity)
                    && let Some(item) = D::get(fetch, entity)
                {
                    return Some(item);
                }
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.candidates.len().saturating_sub(self.pos)))
    }
}
