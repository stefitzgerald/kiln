//! Kiln's entity-component-system.
//!
//! * Entities are generational ids ([`Entity`]).
//! * Components are plain Rust types that implement [`Component`] (a one-line opt-in).
//!   Each component type is stored in its own sparse set, so adding or removing a component
//!   is O(1) and never moves other components.
//! * [`World::query`] iterates entities matching a typed pattern; conflicting borrows are
//!   rejected when the query is created (see [`QueryError`]).
//! * Resources are singletons keyed by type. [`Events`] and [`Commands`] provide messaging
//!   and deferred mutation.
//!
//! Design notes live in `docs/adr/0004-ecs-design.md`.

mod commands;
mod entity;
mod events;
pub mod query;
mod storage;
mod world;

pub use commands::Commands;
pub use entity::Entity;
pub use events::{EventCursor, Events};
pub use query::{
    Has, QueryData, QueryError, QueryFilter, QueryIter, ReadOnlyQueryData, With, Without,
};
pub use world::{EntityError, World};

/// Data that can be attached to an entity.
///
/// Implement it explicitly: `impl Component for Health {}`.
pub trait Component: Send + Sync + 'static {}

/// Global singleton data stored in the [`World`]. Implemented for every `Send + Sync` type.
pub trait Resource: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Resource for T {}

/// A set of components inserted together: a single [`Component`] or a tuple of bundles.
pub trait Bundle: Send + Sync + 'static {
    /// Insert every component into `entity`, which is alive.
    fn insert_into(self, world: &mut World, entity: Entity);
}

impl<C: Component> Bundle for C {
    fn insert_into(self, world: &mut World, entity: Entity) {
        world.insert_one(entity, self);
    }
}

impl Bundle for () {
    fn insert_into(self, _: &mut World, _: Entity) {}
}

macro_rules! impl_bundle_tuple {
    ($($name:ident),+) => {
        impl<$($name: Bundle),+> Bundle for ($($name,)+) {
            #[allow(non_snake_case)]
            fn insert_into(self, world: &mut World, entity: Entity) {
                let ($($name,)+) = self;
                $($name.insert_into(world, entity);)+
            }
        }
    };
}

impl_bundle_tuple!(A);
impl_bundle_tuple!(A, B);
impl_bundle_tuple!(A, B, C);
impl_bundle_tuple!(A, B, C, D);
impl_bundle_tuple!(A, B, C, D, E);
impl_bundle_tuple!(A, B, C, D, E, F);
impl_bundle_tuple!(A, B, C, D, E, F, G);
impl_bundle_tuple!(A, B, C, D, E, F, G, H);
impl_bundle_tuple!(A, B, C, D, E, F, G, H, I);
impl_bundle_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_bundle_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_bundle_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        Bundle, Commands, Component, Entity, EventCursor, Events, Has, Resource, With, Without,
        World,
    };
}
