use kiln_math::{Aabb, Vec3};

/// Error describing an invalid [`Mesh`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    /// A per-vertex attribute has a different length than `positions`.
    #[error("attribute `{attribute}` has {len} entries but the mesh has {vertices} vertices")]
    AttributeLength {
        /// Attribute name.
        attribute: &'static str,
        /// Attribute length.
        len: usize,
        /// Vertex count.
        vertices: usize,
    },
    /// Index count is not a multiple of three.
    #[error("index count {0} is not a multiple of 3")]
    NotTriangles(usize),
    /// An index points past the last vertex.
    #[error("index {index} out of range for {vertices} vertices")]
    IndexOutOfRange {
        /// The offending index value.
        index: u32,
        /// Vertex count.
        vertices: usize,
    },
}

/// Triangle-list mesh with de-interleaved vertex attributes, stored on the CPU.
///
/// Invariant (checked by [`Mesh::validate`]): every attribute has one entry per position.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Unit-length vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates (set 0).
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGBA vertex colors.
    pub colors: Vec<[f32; 4]>,
    /// Triangle list indices.
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Mesh from positions and indices. Normals are computed as flat normals; UVs default to 0
    /// and colors to white.
    pub fn from_positions(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Result<Self, MeshError> {
        let n = positions.len();
        let mut mesh = Self {
            positions,
            normals: vec![[0.0; 3]; n],
            uvs: vec![[0.0; 2]; n],
            colors: vec![[1.0; 4]; n],
            indices,
        };
        mesh.validate()?;
        mesh.compute_flat_normals();
        Ok(mesh)
    }

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Check the mesh invariants.
    pub fn validate(&self) -> Result<(), MeshError> {
        let vertices = self.positions.len();
        for (attribute, len) in
            [("normals", self.normals.len()), ("uvs", self.uvs.len()), ("colors", self.colors.len())]
        {
            if len != vertices {
                return Err(MeshError::AttributeLength { attribute, len, vertices });
            }
        }
        if self.indices.len() % 3 != 0 {
            return Err(MeshError::NotTriangles(self.indices.len()));
        }
        if let Some(&index) = self.indices.iter().find(|&&i| i as usize >= vertices) {
            return Err(MeshError::IndexOutOfRange { index, vertices });
        }
        Ok(())
    }

    /// Bounding box of all positions (`None` for an empty mesh).
    pub fn aabb(&self) -> Option<Aabb> {
        Aabb::from_points(self.positions.iter().map(|p| Vec3::from(*p)))
    }

    /// Replace normals with per-face normals. Vertices are un-shared (the mesh is
    /// de-indexed), as flat shading requires. Degenerate triangles get a +Y normal.
    pub fn compute_flat_normals(&mut self) {
        let mut out = Mesh::default();
        for tri in self.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
            let (pa, pb, pc) =
                (Vec3::from(self.positions[a]), Vec3::from(self.positions[b]), Vec3::from(self.positions[c]));
            let n = (pb - pa).cross(pc - pa).try_normalize().unwrap_or(Vec3::Y);
            for v in [a, b, c] {
                out.indices.push(out.positions.len() as u32);
                out.positions.push(self.positions[v]);
                out.normals.push(n.into());
                out.uvs.push(self.uvs.get(v).copied().unwrap_or_default());
                out.colors.push(self.colors.get(v).copied().unwrap_or([1.0; 4]));
            }
        }
        *self = out;
    }

    /// A single triangle in the XY plane with red, green and blue corners, facing +Z.
    pub fn triangle() -> Self {
        Self {
            positions: vec![[0.0, 0.5, 0.0], [-0.5, -0.5, 0.0], [0.5, -0.5, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]],
            colors: vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]],
            indices: vec![0, 1, 2],
        }
    }

    /// A `size`×`size` quad in the XY plane centered on the origin, facing +Z.
    pub fn quad(size: f32) -> Self {
        let h = size * 0.5;
        Self {
            positions: vec![[-h, -h, 0.0], [h, -h, 0.0], [h, h, 0.0], [-h, h, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    /// A `size`×`size` plane in the XZ plane centered on the origin, facing +Y.
    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        Self {
            positions: vec![[-h, 0.0, h], [h, 0.0, h], [h, 0.0, -h], [-h, 0.0, -h]],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    /// Axis-aligned cube with edge length `size`, 24 vertices (4 per face) and 36 indices.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;
        // (normal, u axis, v axis) per face; corners = n*h ± u*h ± v*h.
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ];
        let mut mesh = Mesh::default();
        for (n, u, v) in faces {
            let (n, u, v) = (Vec3::from(n), Vec3::from(u), Vec3::from(v));
            let base = mesh.positions.len() as u32;
            for (su, sv, uv) in [(-1.0, -1.0, [0.0, 1.0]), (1.0, -1.0, [1.0, 1.0]), (1.0, 1.0, [1.0, 0.0]), (-1.0, 1.0, [0.0, 0.0])] {
                mesh.positions.push(((n + u * su + v * sv) * h).into());
                mesh.normals.push(n.into());
                mesh.uvs.push(uv);
                mesh.colors.push([1.0; 4]);
            }
            mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh
    }

    /// UV sphere with `sectors` longitudinal and `stacks` latitudinal divisions.
    pub fn uv_sphere(radius: f32, sectors: u32, stacks: u32) -> Self {
        let sectors = sectors.max(3);
        let stacks = stacks.max(2);
        let mut mesh = Mesh::default();
        for i in 0..=stacks {
            let v = i as f32 / stacks as f32;
            let phi = std::f32::consts::PI * v; // 0 at +Y
            for j in 0..=sectors {
                let u = j as f32 / sectors as f32;
                let theta = std::f32::consts::TAU * u;
                let n = Vec3::new(phi.sin() * theta.sin(), phi.cos(), phi.sin() * theta.cos());
                mesh.positions.push((n * radius).into());
                mesh.normals.push(n.into());
                mesh.uvs.push([u, v]);
                mesh.colors.push([1.0; 4]);
            }
        }
        let row = sectors + 1;
        for i in 0..stacks {
            for j in 0..sectors {
                let a = i * row + j;
                let b = a + row;
                if i != 0 {
                    mesh.indices.extend_from_slice(&[a, b, a + 1]);
                }
                if i != stacks - 1 {
                    mesh.indices.extend_from_slice(&[a + 1, b, b + 1]);
                }
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_outward_ccw(mesh: &Mesh) {
        // For convex shapes centered at the origin, CCW triangles face away from the center.
        for tri in mesh.indices.chunks_exact(3) {
            let p = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(mesh.positions[i as usize]));
            let face_n = (p[1] - p[0]).cross(p[2] - p[0]);
            let centroid = (p[0] + p[1] + p[2]) / 3.0;
            assert!(face_n.dot(centroid) > 0.0, "inward/cw triangle {tri:?}");
            let vn = Vec3::from(mesh.normals[tri[0] as usize]);
            assert!(vn.dot(face_n) > 0.0, "normal disagrees with winding {tri:?}");
        }
    }

    #[test]
    fn primitives_are_valid_and_wound_ccw() {
        for mesh in [Mesh::cube(1.0), Mesh::uv_sphere(1.0, 16, 8)] {
            mesh.validate().unwrap();
            assert_outward_ccw(&mesh);
        }
        let cube = Mesh::cube(2.0);
        assert_eq!((cube.vertex_count(), cube.indices.len()), (24, 36));
        let aabb = cube.aabb().unwrap();
        assert_eq!((aabb.min, aabb.max), (Vec3::splat(-1.0), Vec3::splat(1.0)));
        for m in [Mesh::triangle(), Mesh::quad(1.0), Mesh::plane(1.0)] {
            m.validate().unwrap();
        }
    }

    /// TC-AST-05 (unit level): flat normals are unit length and perpendicular to faces.
    #[test]
    fn flat_normals() {
        let mesh =
            Mesh::from_positions(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [5.0, 5.0, 5.0]], vec![0, 1, 2, 3, 3, 3])
                .unwrap();
        assert_eq!(mesh.vertex_count(), 6, "de-indexed");
        for n in &mesh.normals {
            assert!((Vec3::from(*n).length() - 1.0).abs() < 1e-6);
        }
        assert_eq!(mesh.normals[0], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.normals[3], [0.0, 1.0, 0.0], "degenerate triangle falls back to +Y");
    }

    #[test]
    fn validation_errors() {
        let mut m = Mesh::triangle();
        m.indices.push(7);
        assert_eq!(m.validate(), Err(MeshError::NotTriangles(4)));
        m.indices.extend([0, 1]);
        assert_eq!(m.validate(), Err(MeshError::IndexOutOfRange { index: 7, vertices: 3 }));
        let mut m = Mesh::triangle();
        m.uvs.pop();
        assert!(matches!(m.validate(), Err(MeshError::AttributeLength { attribute: "uvs", .. })));
    }
}
