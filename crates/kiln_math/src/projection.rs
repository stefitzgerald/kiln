use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec3};

// Clip space uses the DirectX/WebGPU convention (Y-up NDC, depth [0, 1]); the Vulkan backend
// flips Y with a negative viewport height instead of baking a flip into every matrix.

/// Right-handed perspective projection with **reverse-Z** depth: `near` → 1, `far` → 0.
///
/// Reverse-Z spreads floating-point depth precision evenly across the view distance.
/// Pair it with a `GREATER_OR_EQUAL` depth test and a depth clear value of `0.0`.
pub fn perspective_reverse_z(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    // Swapping near/far in a standard [0,1] projection yields reverse-Z.
    directx::perspective(fov_y_radians, aspect, far, near)
}

/// Right-handed perspective with reverse-Z and an infinitely distant far plane.
pub fn perspective_infinite_reverse_z(fov_y_radians: f32, aspect: f32, near: f32) -> Mat4 {
    directx::perspective_infinite_reverse(fov_y_radians, aspect, near)
}

/// Right-handed orthographic projection with reverse-Z depth.
pub fn orthographic_reverse_z(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> Mat4 {
    directx::orthographic(left, right, bottom, top, far, near)
}

/// Right-handed view matrix looking from `eye` toward `target`.
pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    view::look_at_mat4(eye, target, up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn ndc(m: Mat4, p: Vec3) -> Vec3 {
        let c = m * Vec4::new(p.x, p.y, p.z, 1.0);
        c.truncate() / c.w
    }

    /// TC-MATH-02: reverse-Z maps the near plane to 1 and the far plane to 0.
    #[test]
    fn tc_math_02_reverse_z_depth() {
        let p = perspective_reverse_z(60f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
        assert!((ndc(p, Vec3::new(0.0, 0.0, -0.1)).z - 1.0).abs() < 1e-5);
        assert!(ndc(p, Vec3::new(0.0, 0.0, -100.0)).z.abs() < 1e-5);
        // Depth decreases monotonically with distance.
        let mid = ndc(p, Vec3::new(0.0, 0.0, -10.0)).z;
        assert!(mid > 0.0 && mid < 1.0);

        let inf = perspective_infinite_reverse_z(60f32.to_radians(), 1.0, 0.1);
        assert!((ndc(inf, Vec3::new(0.0, 0.0, -0.1)).z - 1.0).abs() < 1e-5);
        assert!(ndc(inf, Vec3::new(0.0, 0.0, -1e7)).z < 1e-6);

        let o = orthographic_reverse_z(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0);
        assert!((ndc(o, Vec3::new(0.0, 0.0, -0.1)).z - 1.0).abs() < 1e-5);
        assert!(ndc(o, Vec3::new(0.0, 0.0, -10.0)).z.abs() < 1e-5);
    }

    /// TC-MATH-03: a camera at (0,0,5) looking at the origin sees −Z forward and +Y up.
    #[test]
    fn tc_math_03_look_at() {
        let view = look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let cam_to_world = view.inverse();
        let forward = cam_to_world.transform_vector3(Vec3::NEG_Z);
        let up = cam_to_world.transform_vector3(Vec3::Y);
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-6), "{forward}");
        assert!(up.abs_diff_eq(Vec3::Y, 1e-6), "{up}");
        // The origin lands 5 units in front of the camera.
        assert!(
            view.transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(0.0, 0.0, -5.0), 1e-6)
        );
    }
}
