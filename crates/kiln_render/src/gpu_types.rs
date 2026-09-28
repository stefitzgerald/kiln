//! CPU mirrors of shader-visible data. Layouts must match `shaders/mesh.wgsl` (std140).

use bytemuck::{Pod, Zeroable};
use kiln_asset::Mesh;
use kiln_math::{Mat3, Mat4, Vec3};

/// Interleaved vertex: 48 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) color: [f32; 4],
}

impl Vertex {
    pub(crate) const STRIDE: u32 = std::mem::size_of::<Self>() as u32;

    pub(crate) fn interleave(mesh: &Mesh) -> Vec<Vertex> {
        (0..mesh.vertex_count())
            .map(|i| Vertex {
                position: mesh.positions[i],
                normal: mesh.normals[i],
                uv: mesh.uvs[i],
                color: mesh.colors[i],
            })
            .collect()
    }
}

/// Per-frame uniforms (`Frame` in the shader).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) camera_pos: [f32; 4],
    pub(crate) light_dir: [f32; 4],
    pub(crate) light_color: [f32; 4],
    pub(crate) ambient: [f32; 4],
}

/// Per-draw uniforms (`Object` in the shader).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct ObjectUniforms {
    pub(crate) model: [[f32; 4]; 4],
    pub(crate) normal: [[f32; 4]; 4],
}

impl ObjectUniforms {
    pub(crate) fn new(model: Mat4) -> Self {
        let m3 = Mat3::from_mat4(model);
        let normal = if m3.determinant().abs() > 1e-12 { m3.inverse().transpose() } else { Mat3::IDENTITY };
        Self { model: model.to_cols_array_2d(), normal: Mat4::from_mat3(normal).to_cols_array_2d() }
    }
}

/// Per-material uniforms (`Material` in the shader).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct MaterialUniforms {
    pub(crate) base_color: [f32; 4],
    pub(crate) flags: [f32; 4],
}

pub(crate) fn vec4(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_shader() {
        assert_eq!(std::mem::size_of::<Vertex>(), 48);
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 128);
        assert_eq!(std::mem::size_of::<ObjectUniforms>(), 128);
        assert_eq!(std::mem::size_of::<MaterialUniforms>(), 32);
    }

    #[test]
    fn normal_matrix_handles_nonuniform_scale() {
        let model = Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0));
        let o = ObjectUniforms::new(model);
        let n = Mat4::from_cols_array_2d(&o.normal);
        // A 45° normal on a surface stretched along X tilts toward Y.
        let v = n.transform_vector3(Vec3::new(1.0, 1.0, 0.0).normalize()).normalize();
        assert!(v.y > v.x);
        // Singular matrices don't produce NaNs.
        let o = ObjectUniforms::new(Mat4::ZERO);
        assert!(o.normal.iter().flatten().all(|f| f.is_finite()));
    }
}
