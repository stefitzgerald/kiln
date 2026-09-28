use glam::{Affine3A, Mat4, Vec3, Vec3A, Vec4};

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Aabb {
    /// Box from corners. Corners are sorted per axis.
    pub fn new(a: Vec3, b: Vec3) -> Self {
        Self { min: a.min(b), max: a.max(b) }
    }

    /// Box from center and half extents.
    pub fn from_center_half_extents(center: Vec3, half: Vec3) -> Self {
        Self { min: center - half.abs(), max: center + half.abs() }
    }

    /// Smallest box enclosing `points`, or `None` if the iterator is empty.
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        Some(it.fold(Self { min: first, max: first }, |b, p| Self {
            min: b.min.min(p),
            max: b.max.max(p),
        }))
    }

    /// Center point.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the size along each axis.
    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// Smallest box containing both.
    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// `true` if `p` is inside or on the boundary.
    pub fn contains_point(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// Smallest axis-aligned box enclosing this box after an affine transform (Arvo's method).
    pub fn transformed(&self, m: &Affine3A) -> Aabb {
        let center = m.transform_point3a(Vec3A::from(self.center()));
        let half = Vec3A::from(self.half_extents());
        let abs = m.matrix3.abs();
        let new_half = abs.x_axis * half.x + abs.y_axis * half.y + abs.z_axis * half.z;
        Aabb { min: (center - new_half).into(), max: (center + new_half).into() }
    }
}

/// Plane `normal · p + d = 0`; the positive half-space is "inside".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Vec3,
    /// Signed offset.
    pub d: f32,
}

impl Plane {
    /// Build from `(a, b, c, d)` coefficients, normalizing. `None` if the normal is ~zero.
    pub fn from_vec4(v: Vec4) -> Option<Self> {
        let n = v.truncate();
        let len = n.length();
        (len > 1e-8).then(|| Self { normal: n / len, d: v.w / len })
    }

    /// Signed distance from the plane to `p`.
    pub fn distance(&self, p: Vec3) -> f32 {
        self.normal.dot(p) + self.d
    }
}

/// Result of a containment test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Containment {
    /// Fully inside.
    Inside,
    /// Fully outside.
    Outside,
    /// Crossing at least one plane.
    Intersect,
}

/// View frustum as up to six inward-facing planes.
#[derive(Debug, Clone, PartialEq)]
pub struct Frustum {
    planes: Vec<Plane>,
}

impl Frustum {
    /// Extract planes from a `projection * view` matrix with `[0, 1]` clip depth (either
    /// standard or reverse-Z). Degenerate planes (e.g. an infinite far plane) are skipped.
    pub fn from_view_projection(m: &Mat4) -> Self {
        let r0 = m.row(0);
        let r1 = m.row(1);
        let r2 = m.row(2);
        let r3 = m.row(3);
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2]
            .into_iter()
            .filter_map(Plane::from_vec4)
            .collect();
        Self { planes }
    }

    /// The frustum's planes.
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// Classify an AABB against the frustum.
    ///
    /// Conservative: boxes near frustum corners may report `Intersect` while actually outside;
    /// they never report `Outside` while visible.
    pub fn classify_aabb(&self, aabb: &Aabb) -> Containment {
        let c = aabb.center();
        let h = aabb.half_extents();
        let mut result = Containment::Inside;
        for plane in &self.planes {
            let r = h.dot(plane.normal.abs());
            let dist = plane.distance(c);
            if dist < -r {
                return Containment::Outside;
            }
            if dist < r {
                result = Containment::Intersect;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{look_at, perspective_reverse_z};
    use glam::Quat;

    /// TC-MATH-04: rotating a box 90° around Y swaps its X and Z extents.
    #[test]
    fn tc_math_04_aabb_rotation() {
        let b = Aabb::new(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0));
        let m = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(90f32.to_radians()),
            Vec3::new(10.0, 0.0, 0.0),
        );
        let t = b.transformed(&m);
        assert!(t.min.abs_diff_eq(Vec3::new(7.0, -2.0, -1.0), 1e-5), "{t:?}");
        assert!(t.max.abs_diff_eq(Vec3::new(13.0, 2.0, 1.0), 1e-5), "{t:?}");

        // 45° grows the box to enclose the rotated corners.
        let unit = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let t = unit.transformed(&Affine3A::from_rotation_y(45f32.to_radians()));
        let s = 2f32.sqrt();
        assert!(t.max.abs_diff_eq(Vec3::new(s, 1.0, s), 1e-5));
    }

    /// TC-MATH-05: frustum classification of inside / outside / straddling boxes.
    #[test]
    fn tc_math_05_frustum_classify() {
        let proj = perspective_reverse_z(90f32.to_radians(), 1.0, 0.1, 100.0);
        let view = look_at(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let f = Frustum::from_view_projection(&(proj * view));
        assert_eq!(f.planes().len(), 6);

        let unit = |c: Vec3| Aabb::from_center_half_extents(c, Vec3::splat(0.5));
        assert_eq!(f.classify_aabb(&unit(Vec3::new(0.0, 0.0, -10.0))), Containment::Inside);
        assert_eq!(f.classify_aabb(&unit(Vec3::new(0.0, 0.0, 10.0))), Containment::Outside);
        assert_eq!(f.classify_aabb(&unit(Vec3::new(0.0, 0.0, -200.0))), Containment::Outside);
        assert_eq!(f.classify_aabb(&unit(Vec3::new(50.0, 0.0, -10.0))), Containment::Outside);
        // Straddles the right plane (x = -z at 90° fov).
        assert_eq!(f.classify_aabb(&unit(Vec3::new(10.0, 0.0, -10.0))), Containment::Intersect);
        // Straddles the far plane.
        assert_eq!(f.classify_aabb(&unit(Vec3::new(0.0, 0.0, -100.0))), Containment::Intersect);
    }

    #[test]
    fn infinite_projection_skips_far_plane() {
        let proj = crate::perspective_infinite_reverse_z(1.0, 1.0, 0.1);
        let f = Frustum::from_view_projection(&proj);
        assert_eq!(f.planes().len(), 5);
        let far = Aabb::from_center_half_extents(Vec3::new(0.0, 0.0, -1e6), Vec3::ONE);
        assert_eq!(f.classify_aabb(&far), Containment::Inside);
    }

    #[test]
    fn from_points_and_union() {
        assert!(Aabb::from_points(std::iter::empty()).is_none());
        let b = Aabb::from_points([Vec3::ONE, Vec3::NEG_ONE, Vec3::X * 3.0]).unwrap();
        assert_eq!(b.min, Vec3::NEG_ONE);
        assert_eq!(b.max, Vec3::new(3.0, 1.0, 1.0));
        assert!(b.contains_point(Vec3::ZERO));
        let u = b.union(&Aabb::new(Vec3::splat(5.0), Vec3::splat(6.0)));
        assert_eq!(u.max, Vec3::splat(6.0));
    }
}
