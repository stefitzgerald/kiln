use glam::{Affine3A, Mat3, Mat4, Quat, Vec3};

/// Translation, rotation and scale. Applied as scale, then rotate, then translate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Position.
    pub translation: Vec3,
    /// Orientation (must be normalized).
    pub rotation: Quat,
    /// Per-axis scale.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self =
        Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    /// Pure translation.
    pub const fn from_translation(translation: Vec3) -> Self {
        Self { translation, ..Self::IDENTITY }
    }

    /// Pure translation from components.
    pub const fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self::from_translation(Vec3::new(x, y, z))
    }

    /// Pure rotation.
    pub const fn from_rotation(rotation: Quat) -> Self {
        Self { rotation, ..Self::IDENTITY }
    }

    /// Pure scale.
    pub const fn from_scale(scale: Vec3) -> Self {
        Self { scale, ..Self::IDENTITY }
    }

    /// Decompose an affine matrix. Shear is lost.
    pub fn from_affine(affine: Affine3A) -> Self {
        let (scale, rotation, translation) = affine.to_scale_rotation_translation();
        Self { translation, rotation, scale }
    }

    /// Builder: set translation.
    pub fn with_translation(mut self, translation: Vec3) -> Self {
        self.translation = translation;
        self
    }

    /// Builder: set rotation.
    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    /// Builder: set scale.
    pub fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    /// Rotate so that local −Z points at `target`, keeping `up` as close to local +Y as possible.
    /// Leaves the transform unchanged if `target` coincides with the translation.
    pub fn looking_at(mut self, target: Vec3, up: Vec3) -> Self {
        let forward = target - self.translation;
        if forward.length_squared() < 1e-12 {
            return self;
        }
        let back = -forward.normalize();
        let mut right = up.cross(back);
        if right.length_squared() < 1e-12 {
            // `up` is parallel to the view direction; pick any perpendicular axis.
            right = back.any_orthonormal_vector();
        }
        let right = right.normalize();
        let up = back.cross(right);
        self.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, back)).normalize();
        self
    }

    /// Local forward direction (−Z) in parent space.
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// Local right direction (+X) in parent space.
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// Local up direction (+Y) in parent space.
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// As an affine matrix.
    pub fn to_affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// As a 4×4 matrix.
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// Transform a point.
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.translation + self.rotation * (self.scale * point)
    }

    /// Compose: `self * child` (apply `child` first, then `self`).
    ///
    /// Exact when `self` has uniform scale; with non-uniform scale and a rotated child the
    /// result would need shear, so use [`Transform::to_affine`] products for that case.
    pub fn mul_transform(&self, child: &Transform) -> Transform {
        Transform {
            translation: self.transform_point(child.translation),
            rotation: (self.rotation * child.rotation).normalize(),
            scale: self.scale * child.scale,
        }
    }

    /// Inverse transform. Exact for uniform scale (see [`Transform::mul_transform`]).
    pub fn inverse(&self) -> Transform {
        let inv_rot = self.rotation.inverse();
        let inv_scale = self.scale.recip();
        Transform {
            translation: inv_rot * (-self.translation) * inv_scale,
            rotation: inv_rot,
            scale: inv_scale,
        }
    }
}

impl std::ops::Mul for Transform {
    type Output = Transform;
    fn mul(self, rhs: Transform) -> Transform {
        self.mul_transform(&rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_vec3(range: f32) -> impl Strategy<Value = Vec3> {
        (-range..range, -range..range, -range..range).prop_map(|(x, y, z)| Vec3::new(x, y, z))
    }

    fn arb_quat() -> impl Strategy<Value = Quat> {
        (arb_vec3(1.0), -std::f32::consts::PI..std::f32::consts::PI).prop_filter_map(
            "degenerate axis",
            |(axis, angle)| (axis.length() > 1e-3).then(|| Quat::from_axis_angle(axis.normalize(), angle)),
        )
    }

    proptest! {
        /// TC-MATH-01: T * T⁻¹ == identity (uniform scale).
        #[test]
        fn tc_math_01_inverse_roundtrip(t in arb_vec3(100.0), r in arb_quat(), s in 0.1f32..10.0) {
            let tf = Transform { translation: t, rotation: r, scale: Vec3::splat(s) };
            let id = (tf * tf.inverse()).to_matrix();
            prop_assert!(id.abs_diff_eq(Mat4::IDENTITY, 1e-4), "{id:?}");
            let id2 = (tf.inverse() * tf).to_matrix();
            prop_assert!(id2.abs_diff_eq(Mat4::IDENTITY, 1e-4), "{id2:?}");
        }

        /// Non-uniform scale goes through the affine path, which is always exact.
        #[test]
        fn affine_inverse_roundtrip(t in arb_vec3(100.0), r in arb_quat(), s in arb_vec3(10.0)) {
            prop_assume!(s.abs().min_element() > 0.1);
            let a = Transform { translation: t, rotation: r, scale: s }.to_affine();
            let id = Mat4::from(a * a.inverse());
            prop_assert!(id.abs_diff_eq(Mat4::IDENTITY, 1e-3));
        }

        /// Transform composition agrees with matrix multiplication.
        #[test]
        fn mul_matches_matrix(t1 in arb_vec3(10.0), r1 in arb_quat(), s1 in 0.1f32..4.0,
                              t2 in arb_vec3(10.0), r2 in arb_quat(), s2 in arb_vec3(4.0)) {
            prop_assume!(s2.abs().min_element() > 0.1);
            let a = Transform { translation: t1, rotation: r1, scale: Vec3::splat(s1) };
            let b = Transform { translation: t2, rotation: r2, scale: s2 };
            prop_assert!((a * b).to_matrix().abs_diff_eq(a.to_matrix() * b.to_matrix(), 1e-3));
        }
    }

    #[test]
    fn looking_at_points_forward() {
        let t = Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y);
        assert!(t.forward().abs_diff_eq(Vec3::NEG_Z, 1e-6));
        let t = Transform::from_xyz(3.0, 0.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y);
        assert!(t.forward().abs_diff_eq(Vec3::NEG_X, 1e-6));
        assert!(t.up().abs_diff_eq(Vec3::Y, 1e-6));
        // Degenerate up vector must still yield a valid rotation.
        let t = Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y);
        assert!(t.forward().abs_diff_eq(Vec3::NEG_Y, 1e-6));
        assert!(t.rotation.is_normalized());
    }
}
