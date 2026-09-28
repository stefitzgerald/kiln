//! Math types for Kiln.
//!
//! Conventions (see `docs/adr/0003-coordinate-system.md`):
//! * Right-handed, **Y-up**, **−Z forward**, units in meters.
//! * Clip-space depth is `[0, 1]` with **reverse-Z** (near plane → 1, far plane → 0).
//!
//! `glam` is re-exported; engine-specific types live alongside it.

mod bounds;
mod projection;
mod transform;

pub use bounds::{Aabb, Containment, Frustum, Plane};
pub use glam;
pub use glam::{Affine3A, EulerRot, Mat3, Mat3A, Mat4, Quat, UVec2, Vec2, Vec3, Vec3A, Vec4};
pub use projection::{look_at, perspective_reverse_z, perspective_infinite_reverse_z, orthographic_reverse_z};
pub use transform::Transform;

/// World up axis (+Y).
pub const UP: Vec3 = Vec3::Y;
/// Default forward axis (−Z).
pub const FORWARD: Vec3 = Vec3::NEG_Z;
/// Default right axis (+X).
pub const RIGHT: Vec3 = Vec3::X;
