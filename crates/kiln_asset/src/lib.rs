//! CPU-side assets: [`Mesh`], [`Material`], [`Image`] and glTF 2.0 import.
//!
//! Assets are stored in typed [`Assets<T>`] collections and referenced with
//! [`Handle<T>`](kiln_core::Handle). The [`AssetServer`] resource owns all collections and
//! de-duplicates loads by canonical path.
//!
//! Loading is synchronous in M0; background loading is planned for a later milestone.

mod gltf_loader;
mod image;
mod material;
mod mesh;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use gltf_loader::{GltfAsset, GltfMesh, GltfNode, GltfPrimitive, GltfScene};
pub use image::{ColorSpace, Image};
pub use kiln_core::Handle;
pub use material::Material;
pub use mesh::{Mesh, MeshError};

/// Errors produced while loading assets.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// The file does not exist.
    #[error("asset not found: {}", path.display())]
    NotFound {
        /// Requested path.
        path: PathBuf,
    },
    /// The file exists but could not be read.
    #[error("failed to read {}: {source}", path.display())]
    Io {
        /// Requested path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The data is malformed or uses unsupported features.
    #[error("failed to parse {origin}: {message}")]
    Parse {
        /// File path or `"<memory>"`.
        origin: String,
        /// What went wrong.
        message: String,
    },
}

/// A typed collection of assets.
#[derive(Debug)]
pub struct Assets<T> {
    pool: kiln_core::HandlePool<T>,
}

impl<T> Default for Assets<T> {
    fn default() -> Self {
        Self { pool: kiln_core::HandlePool::new() }
    }
}

impl<T> Assets<T> {
    /// Add an asset.
    pub fn add(&mut self, asset: T) -> Handle<T> {
        self.pool.insert(asset)
    }

    /// Look up an asset.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.pool.get(handle)
    }

    /// Mutably look up an asset.
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.pool.get_mut(handle)
    }

    /// Remove an asset.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        self.pool.remove(handle)
    }

    /// Number of assets.
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// `true` if empty.
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }

    /// Iterate `(handle, asset)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.pool.iter()
    }
}

/// Owns every asset collection and loads files.
#[derive(Debug, Default)]
pub struct AssetServer {
    /// Meshes.
    pub meshes: Assets<Mesh>,
    /// Materials.
    pub materials: Assets<Material>,
    /// Images.
    pub images: Assets<Image>,
    /// Imported glTF documents.
    pub gltfs: Assets<GltfAsset>,
    by_path: HashMap<PathBuf, Handle<GltfAsset>>,
    loads: usize,
}

impl AssetServer {
    /// Empty server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a `.gltf` or `.glb` file. Loading the same file again returns the cached handle.
    pub fn load_gltf(&mut self, path: impl AsRef<Path>) -> Result<Handle<GltfAsset>, AssetError> {
        let path = path.as_ref();
        let canonical = std::fs::canonicalize(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AssetError::NotFound { path: path.to_owned() },
            _ => AssetError::Io { path: path.to_owned(), source: e },
        })?;
        if let Some(&handle) = self.by_path.get(&canonical) {
            if self.gltfs.get(handle).is_some() {
                return Ok(handle);
            }
        }
        let bytes = std::fs::read(&canonical)
            .map_err(|e| AssetError::Io { path: path.to_owned(), source: e })?;
        let origin = path.display().to_string();
        let asset = gltf_loader::load(&bytes, canonical.parent(), &origin, self)?;
        let handle = self.gltfs.add(asset);
        self.by_path.insert(canonical, handle);
        self.loads += 1;
        tracing::info!(path = %origin, "loaded glTF");
        Ok(handle)
    }

    /// Load glTF data from memory. External URIs are resolved against `base_dir`, if given.
    /// Not cached.
    pub fn load_gltf_from_bytes(
        &mut self,
        bytes: &[u8],
        base_dir: Option<&Path>,
    ) -> Result<Handle<GltfAsset>, AssetError> {
        let asset = gltf_loader::load(bytes, base_dir, "<memory>", self)?;
        self.loads += 1;
        Ok(self.gltfs.add(asset))
    }

    /// Number of loads that actually read and parsed data (cache hits excluded).
    pub fn load_count(&self) -> usize {
        self.loads
    }
}

/// Commonly used items.
pub mod prelude {
    pub use crate::{AssetServer, Assets, ColorSpace, GltfAsset, Handle, Image, Material, Mesh};
}
