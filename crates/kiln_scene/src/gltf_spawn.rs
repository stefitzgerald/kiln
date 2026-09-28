use kiln_asset::{AssetServer, GltfAsset, Handle};
use kiln_ecs::{Entity, World};

use crate::{MeshInstance, Name, Transform, set_parent};

/// Error from [`spawn_gltf_scene`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    /// The world has no [`AssetServer`] resource.
    #[error("no AssetServer resource in the world")]
    NoAssetServer,
    /// The glTF handle is stale.
    #[error("glTF asset {0:?} is not loaded")]
    NotLoaded(Handle<GltfAsset>),
    /// The requested scene index does not exist.
    #[error("glTF asset has no scene {0}")]
    NoSuchScene(usize),
    /// A node index in the document is out of range.
    #[error("glTF node {0} does not exist")]
    BadNode(usize),
}

/// Spawn a glTF scene (the default scene if `scene` is `None`) under a new root entity,
/// mirroring the node hierarchy. Returns the root entity.
///
/// A node whose mesh has one primitive gets a [`MeshInstance`] directly; a mesh with
/// several primitives gets one child entity per primitive.
pub fn spawn_gltf_scene(
    world: &mut World,
    gltf: Handle<GltfAsset>,
    scene: Option<usize>,
) -> Result<Entity, SceneError> {
    let server = world.resource::<AssetServer>().ok_or(SceneError::NoAssetServer)?;
    let asset = server.gltfs.get(gltf).ok_or(SceneError::NotLoaded(gltf))?.clone();
    let roots = match scene.or(asset.default_scene) {
        Some(i) => asset.scenes.get(i).ok_or(SceneError::NoSuchScene(i))?.roots.clone(),
        None => Vec::new(),
    };

    let root = world.spawn((Transform::IDENTITY, Name::new("glTF scene")));
    // Iterative DFS: (node index, parent entity, depth guard).
    let mut stack: Vec<(usize, Entity, usize)> = roots.iter().rev().map(|&n| (n, root, 0)).collect();
    while let Some((index, parent, depth)) = stack.pop() {
        // A valid glTF node graph is a forest; guard anyway against malformed cycles.
        if depth > asset.nodes.len() {
            return Err(SceneError::BadNode(index));
        }
        let node = asset.nodes.get(index).ok_or(SceneError::BadNode(index))?;
        let name = node.name.clone().unwrap_or_else(|| format!("node {index}"));
        let entity = world.spawn((Transform(node.transform), Name(name)));
        set_parent(world, entity, parent).map_err(|_| SceneError::BadNode(index))?;

        if let Some(mesh) = node.mesh.and_then(|m| asset.meshes.get(m)) {
            if let [prim] = mesh.primitives.as_slice() {
                let _ = world.insert(entity, MeshInstance { mesh: prim.mesh, material: prim.material });
            } else {
                for (i, prim) in mesh.primitives.iter().enumerate() {
                    let child = world.spawn((
                        Transform::IDENTITY,
                        Name(format!("primitive {i}")),
                        MeshInstance { mesh: prim.mesh, material: prim.material },
                    ));
                    set_parent(world, child, entity).map_err(|_| SceneError::BadNode(index))?;
                }
            }
        }
        stack.extend(node.children.iter().rev().map(|&c| (c, entity, depth + 1)));
    }
    Ok(root)
}
