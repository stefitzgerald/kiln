use std::ops::{Deref, DerefMut};

use kiln_asset::{Handle, Material, Mesh};
use kiln_ecs::Component;
use kiln_math::{Affine3A, Mat4, Quat, Vec3};

/// Local transform relative to the parent (or the world, for roots).
///
/// Wraps [`kiln_math::Transform`] and dereferences to it, so fields and methods such as
/// `translation` or `looking_at` are available directly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transform(pub kiln_math::Transform);

impl Component for Transform {}

impl Deref for Transform {
    type Target = kiln_math::Transform;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Transform {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<kiln_math::Transform> for Transform {
    fn from(t: kiln_math::Transform) -> Self {
        Self(t)
    }
}

impl Transform {
    /// Identity.
    pub const IDENTITY: Self = Self(kiln_math::Transform::IDENTITY);

    /// Pure translation.
    pub const fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self(kiln_math::Transform::from_xyz(x, y, z))
    }

    /// Pure translation.
    pub const fn from_translation(t: Vec3) -> Self {
        Self(kiln_math::Transform::from_translation(t))
    }

    /// Pure rotation.
    pub const fn from_rotation(r: Quat) -> Self {
        Self(kiln_math::Transform::from_rotation(r))
    }

    /// Pure scale.
    pub const fn from_scale(s: Vec3) -> Self {
        Self(kiln_math::Transform::from_scale(s))
    }

    /// Builder: rotate to look at `target`.
    pub fn looking_at(self, target: Vec3, up: Vec3) -> Self {
        Self(self.0.looking_at(target, up))
    }

    /// Builder: set rotation.
    pub fn with_rotation(self, r: Quat) -> Self {
        Self(self.0.with_rotation(r))
    }

    /// Builder: set scale.
    pub fn with_scale(self, s: Vec3) -> Self {
        Self(self.0.with_scale(s))
    }
}

/// World-space transform, computed by transform propagation. Do not set it manually.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GlobalTransform(pub Affine3A);

impl Component for GlobalTransform {}

impl GlobalTransform {
    /// As a 4×4 matrix.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from(self.0)
    }

    /// World-space position.
    pub fn translation(&self) -> Vec3 {
        self.0.translation.into()
    }

    /// World-space forward direction (−Z), normalized.
    pub fn forward(&self) -> Vec3 {
        self.0.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z)
    }
}

/// Human-readable entity name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Name(pub String);

impl Component for Name {}

impl Name {
    /// New name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// Renders a mesh with a material at the entity's [`GlobalTransform`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshInstance {
    /// Geometry.
    pub mesh: Handle<Mesh>,
    /// Surface.
    pub material: Handle<Material>,
}

impl Component for MeshInstance {}

/// Camera projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Perspective with reverse-Z depth.
    Perspective {
        /// Vertical field of view in radians.
        fov_y: f32,
        /// Near plane distance (> 0).
        near: f32,
        /// Far plane distance; `f32::INFINITY` for an infinite far plane.
        far: f32,
    },
    /// Orthographic with reverse-Z depth.
    Orthographic {
        /// Visible height in world units; width follows the aspect ratio.
        height: f32,
        /// Near plane distance.
        near: f32,
        /// Far plane distance.
        far: f32,
    },
}

impl Default for Projection {
    fn default() -> Self {
        Projection::Perspective { fov_y: 60f32.to_radians(), near: 0.1, far: 1000.0 }
    }
}

impl Projection {
    /// Projection matrix for a target with the given aspect ratio (width / height).
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        match *self {
            Projection::Perspective { fov_y, near, far } if far.is_infinite() => {
                kiln_math::perspective_infinite_reverse_z(fov_y, aspect, near)
            }
            Projection::Perspective { fov_y, near, far } => {
                kiln_math::perspective_reverse_z(fov_y, aspect, near, far)
            }
            Projection::Orthographic { height, near, far } => {
                let h = height * 0.5;
                let w = h * aspect;
                kiln_math::orthographic_reverse_z(-w, w, -h, h, near, far)
            }
        }
    }
}

/// Renders the scene from the entity's [`GlobalTransform`], looking down its −Z axis.
/// The first active camera found is used.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Camera {
    /// Projection.
    pub projection: Projection,
    /// Inactive cameras are ignored.
    pub inactive: bool,
}

impl Component for Camera {}

impl Camera {
    /// View matrix (world → camera) for a camera placed at `global`.
    pub fn view_matrix(global: &GlobalTransform) -> Mat4 {
        Mat4::from(global.0.inverse())
    }

    /// `projection * view`.
    pub fn view_projection(&self, global: &GlobalTransform, aspect: f32) -> Mat4 {
        self.projection.matrix(aspect) * Self::view_matrix(global)
    }
}

/// Sun-like light shining along the entity's −Z axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    /// Linear RGB color.
    pub color: [f32; 3],
    /// Intensity multiplier.
    pub intensity: f32,
}

impl Component for DirectionalLight {}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self { color: [1.0; 3], intensity: 1.0 }
    }
}

/// Uniform ambient light (resource).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientLight {
    /// Linear RGB color.
    pub color: [f32; 3],
    /// Intensity multiplier.
    pub intensity: f32,
}

impl Default for AmbientLight {
    fn default() -> Self {
        Self { color: [1.0; 3], intensity: 0.15 }
    }
}

/// Background color (resource), linear RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearColor(pub [f32; 4]);

impl Default for ClearColor {
    fn default() -> Self {
        // A neutral dark gray-blue.
        Self([0.02, 0.025, 0.035, 1.0])
    }
}
