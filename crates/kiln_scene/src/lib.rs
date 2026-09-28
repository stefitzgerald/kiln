//! Scene components and systems: transforms, hierarchy, cameras, lights, and spawning of
//! imported glTF scenes.
//!
//! * [`Transform`] is an entity's local transform; [`GlobalTransform`] is computed from it
//!   by [`propagate_transforms`], which [`ScenePlugin`] runs in [`Stage::PostUpdate`].
//! * Hierarchy is stored as [`Parent`] / [`Children`] and must be edited through
//!   [`set_parent`], [`remove_parent`] and [`despawn_recursive`], which keep both sides
//!   consistent and reject cycles.

mod components;
mod gltf_spawn;
mod hierarchy;

pub use components::{
    AmbientLight, Camera, ClearColor, DirectionalLight, GlobalTransform, MeshInstance, Name,
    Projection, Transform,
};
pub use gltf_spawn::{SceneError, spawn_gltf_scene};
pub use hierarchy::{
    Children, HierarchyError, Parent, despawn_recursive, propagate_transforms, remove_parent,
    set_parent,
};

use kiln_app::{App, Plugin, Stage};
use kiln_asset::AssetServer;

/// Registers scene resources ([`AssetServer`], [`AmbientLight`], [`ClearColor`]) and runs
/// transform propagation every frame.
#[derive(Debug, Default)]
pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetServer>()
            .init_resource::<AmbientLight>()
            .init_resource::<ClearColor>()
            .add_system(Stage::PostUpdate, propagate_transforms);
    }
}

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        AmbientLight, Camera, Children, ClearColor, DirectionalLight, GlobalTransform,
        MeshInstance, Name, Parent, Projection, ScenePlugin, Transform, despawn_recursive,
        set_parent, spawn_gltf_scene,
    };
}
