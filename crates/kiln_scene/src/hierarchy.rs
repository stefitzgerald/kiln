use kiln_ecs::{Component, Entity, World};
use kiln_math::Affine3A;

use crate::{GlobalTransform, Transform};

/// The entity's parent. Edit through [`set_parent`] / [`remove_parent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent(pub Entity);

impl Component for Parent {}

/// The entity's children, in insertion order. Edit through [`set_parent`] / [`remove_parent`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Children(pub Vec<Entity>);

impl Component for Children {}

/// Error from hierarchy edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HierarchyError {
    /// Parent or child is not alive.
    #[error("entity {0} does not exist")]
    NoSuchEntity(Entity),
    /// The edit would make an entity its own ancestor.
    #[error("making {parent} the parent of {child} would create a cycle")]
    Cycle {
        /// Requested child.
        child: Entity,
        /// Requested parent.
        parent: Entity,
    },
}

/// Make `parent` the parent of `child`, detaching `child` from any previous parent.
/// On error the hierarchy is unchanged.
pub fn set_parent(world: &mut World, child: Entity, parent: Entity) -> Result<(), HierarchyError> {
    for e in [child, parent] {
        if !world.is_alive(e) {
            return Err(HierarchyError::NoSuchEntity(e));
        }
    }
    // Walk up from `parent`; reaching `child` means `parent` is a descendant of `child`.
    let mut cursor = Some(parent);
    while let Some(e) = cursor {
        if e == child {
            return Err(HierarchyError::Cycle { child, parent });
        }
        cursor = world.get::<Parent>(e).map(|p| p.0);
    }
    detach_from_parent(world, child);
    // Both entities were checked alive above, so these inserts cannot fail.
    let _ = world.insert(child, Parent(parent));
    match world.get_mut::<Children>(parent) {
        Some(children) => children.0.push(child),
        None => {
            let _ = world.insert(parent, Children(vec![child]));
        }
    }
    Ok(())
}

/// Detach `child` from its parent, making it a root. No-op if it has no parent.
pub fn remove_parent(world: &mut World, child: Entity) {
    detach_from_parent(world, child);
    world.remove::<Parent>(child);
}

fn detach_from_parent(world: &mut World, child: Entity) {
    let Some(Parent(old)) = world.get::<Parent>(child).copied() else {
        return;
    };
    if let Some(children) = world.get_mut::<Children>(old) {
        children.0.retain(|&c| c != child);
        if children.0.is_empty() {
            world.remove::<Children>(old);
        }
    }
}

/// Despawn `entity` and all its descendants, and detach it from its parent.
/// Returns the number of entities despawned.
pub fn despawn_recursive(world: &mut World, entity: Entity) -> usize {
    if !world.is_alive(entity) {
        return 0;
    }
    detach_from_parent(world, entity);
    let mut stack = vec![entity];
    let mut count = 0;
    while let Some(e) = stack.pop() {
        if let Some(children) = world.get::<Children>(e) {
            stack.extend_from_slice(&children.0);
        }
        if world.despawn(e) {
            count += 1;
        }
    }
    count
}

/// Compute [`GlobalTransform`] for every entity that has a [`Transform`], walking the
/// hierarchy from its roots. Entities without a `Transform` inside a hierarchy act as
/// identity. Entities whose parent was despawned are treated as roots.
pub fn propagate_transforms(world: &mut World) {
    let mut stack: Vec<(Entity, Affine3A)> = world
        .query_ref::<(Entity, Option<&Parent>), kiln_ecs::With<Transform>>()
        .filter(|(_, parent)| parent.is_none_or(|p| !world.is_alive(p.0)))
        .map(|(e, _)| (e, Affine3A::IDENTITY))
        .collect();
    while let Some((e, parent_global)) = stack.pop() {
        let local = world.get::<Transform>(e).map_or(Affine3A::IDENTITY, |t| t.to_affine());
        let global = parent_global * local;
        match world.get_mut::<GlobalTransform>(e) {
            Some(g) => g.0 = global,
            None => {
                let _ = world.insert(e, GlobalTransform(global));
            }
        }
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.0.iter().map(|&c| (c, global)));
        }
    }
}
