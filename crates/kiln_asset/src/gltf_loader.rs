//! glTF 2.0 import (`.gltf` with embedded or external buffers, and `.glb`).

use std::path::Path;

use gltf::mesh::Mode;
use kiln_core::Handle;
use kiln_math::{Quat, Transform, Vec3};

use crate::{AssetError, AssetServer, ColorSpace, Image, Material, Mesh};

/// One drawable part of a glTF mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GltfPrimitive {
    /// Geometry.
    pub mesh: Handle<Mesh>,
    /// Surface.
    pub material: Handle<Material>,
}

/// A glTF mesh: a list of primitives.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfMesh {
    /// Optional name.
    pub name: Option<String>,
    /// Primitives, in file order.
    pub primitives: Vec<GltfPrimitive>,
}

/// A node of the glTF scene graph.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfNode {
    /// Optional name.
    pub name: Option<String>,
    /// Local transform relative to the parent.
    pub transform: Transform,
    /// Index into [`GltfAsset::meshes`].
    pub mesh: Option<usize>,
    /// Indices into [`GltfAsset::nodes`].
    pub children: Vec<usize>,
}

/// A glTF scene: a set of root nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfScene {
    /// Optional name.
    pub name: Option<String>,
    /// Root node indices.
    pub roots: Vec<usize>,
}

/// An imported glTF document. Meshes, materials and images are stored in the
/// [`AssetServer`] collections; this struct keeps the structure that ties them together.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfAsset {
    /// Meshes, indexed like the source file.
    pub meshes: Vec<GltfMesh>,
    /// Materials, indexed like the source file.
    pub materials: Vec<Handle<Material>>,
    /// Images, indexed like the source file.
    pub images: Vec<Handle<Image>>,
    /// Nodes, indexed like the source file.
    pub nodes: Vec<GltfNode>,
    /// Scenes.
    pub scenes: Vec<GltfScene>,
    /// Scene to show by default (the file's `scene`, else the first).
    pub default_scene: Option<usize>,
}

impl GltfAsset {
    /// Root nodes of the default scene.
    pub fn default_roots(&self) -> &[usize] {
        self.default_scene.and_then(|i| self.scenes.get(i)).map_or(&[], |s| &s.roots)
    }
}

pub(crate) fn load(
    bytes: &[u8],
    base: Option<&Path>,
    origin: &str,
    server: &mut AssetServer,
) -> Result<GltfAsset, AssetError> {
    let parse = |message: String| AssetError::Parse { origin: origin.to_owned(), message };

    // The gltf crate validates the document, but indexing into malformed buffers can still
    // panic in dependencies. Asset files are untrusted input, so contain any panic here.
    let decoded = std::panic::catch_unwind(|| decode(bytes, base))
        .map_err(|_| parse("internal decoder panic (malformed file)".into()))?
        .map_err(parse)?;

    let mut asset = GltfAsset {
        meshes: Vec::new(),
        materials: Vec::new(),
        images: Vec::new(),
        nodes: decoded.nodes,
        scenes: decoded.scenes,
        default_scene: decoded.default_scene,
    };
    for image in decoded.images {
        asset.images.push(server.images.add(image));
    }
    for mut material in decoded.materials {
        material.base_color_texture = material
            .base_color_texture
            .and_then(|h| asset.images.get(h.index() as usize).copied());
        asset.materials.push(server.materials.add(material));
    }
    let mut default_material = None;
    for mesh in decoded.meshes {
        let mut primitives = Vec::with_capacity(mesh.primitives.len());
        for (cpu, material) in mesh.primitives {
            let material = match material {
                Some(i) => asset.materials[i],
                None => *default_material
                    .get_or_insert_with(|| server.materials.add(Material::default())),
            };
            primitives.push(GltfPrimitive { mesh: server.meshes.add(cpu), material });
        }
        asset.meshes.push(GltfMesh { name: mesh.name, primitives });
    }
    Ok(asset)
}

struct DecodedMesh {
    name: Option<String>,
    primitives: Vec<(Mesh, Option<usize>)>,
}

/// Everything parsed out of the file, before handles are assigned. Material texture handles
/// temporarily hold the source image index.
struct Decoded {
    images: Vec<Image>,
    materials: Vec<Material>,
    meshes: Vec<DecodedMesh>,
    nodes: Vec<GltfNode>,
    scenes: Vec<GltfScene>,
    default_scene: Option<usize>,
}

fn decode(bytes: &[u8], base: Option<&Path>) -> Result<Decoded, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let buffers = gltf::import_buffers(&document, base, blob).map_err(|e| e.to_string())?;
    let raw_images = gltf::import_images(&document, base, &buffers).map_err(|e| e.to_string())?;

    // Images referenced as base color are sRGB; everything else is linear data.
    let mut srgb = vec![false; raw_images.len()];
    for m in document.materials() {
        if let Some(info) = m.pbr_metallic_roughness().base_color_texture() {
            srgb[info.texture().source().index()] = true;
        }
    }
    let images = raw_images
        .into_iter()
        .zip(srgb)
        .map(|(data, is_srgb)| {
            convert_image(data, if is_srgb { ColorSpace::Srgb } else { ColorSpace::Linear })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let materials = document
        .materials()
        .map(|m| {
            let pbr = m.pbr_metallic_roughness();
            Material {
                name: m.name().map(str::to_owned),
                base_color: pbr.base_color_factor(),
                base_color_texture: pbr
                    .base_color_texture()
                    .map(|t| Handle::from_raw_parts(t.texture().source().index() as u32, 0)),
                unlit: m.unlit(),
                double_sided: m.double_sided(),
            }
        })
        .collect();

    let mut meshes = Vec::new();
    for mesh in document.meshes() {
        let mut primitives = Vec::new();
        for prim in mesh.primitives() {
            match read_primitive(&prim, &buffers)? {
                Some(cpu) => primitives.push((cpu, prim.material().index())),
                None => tracing::warn!(
                    mesh = mesh.index(),
                    mode = ?prim.mode(),
                    "skipping primitive with unsupported mode"
                ),
            }
        }
        meshes.push(DecodedMesh { name: mesh.name().map(str::to_owned), primitives });
    }

    let nodes = document
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            GltfNode {
                name: n.name().map(str::to_owned),
                transform: Transform {
                    translation: Vec3::from(t),
                    rotation: Quat::from_array(r).normalize(),
                    scale: Vec3::from(s),
                },
                mesh: n.mesh().map(|m| m.index()),
                children: n.children().map(|c| c.index()).collect(),
            }
        })
        .collect();

    let scenes = document
        .scenes()
        .map(|s| GltfScene {
            name: s.name().map(str::to_owned),
            roots: s.nodes().map(|n| n.index()).collect(),
        })
        .collect::<Vec<_>>();
    let default_scene =
        document.default_scene().map(|s| s.index()).or((!scenes.is_empty()).then_some(0));

    Ok(Decoded { images, materials, meshes, nodes, scenes, default_scene })
}

/// Read one primitive as a triangle list. `Ok(None)` for point/line primitives.
fn read_primitive(
    prim: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<Option<Mesh>, String> {
    let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| format!("mesh primitive {} has no POSITION", prim.index()))?
        .collect();
    let n = positions.len();
    let raw_indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..n as u32).collect(),
    };
    if let Some(bad) = raw_indices.iter().find(|&&i| i as usize >= n) {
        return Err(format!("index {bad} out of range for {n} vertices"));
    }
    let indices = match prim.mode() {
        Mode::Triangles => raw_indices,
        Mode::TriangleStrip => strip_to_list(&raw_indices),
        Mode::TriangleFan => fan_to_list(&raw_indices),
        Mode::Points | Mode::Lines | Mode::LineLoop | Mode::LineStrip => return Ok(None),
    };
    let fill = |len: usize, what: &str| {
        if len == n { Ok(()) } else { Err(format!("{what} count {len} != position count {n}")) }
    };
    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
        Some(t) => t.into_f32().collect(),
        None => vec![[0.0; 2]; n],
    };
    fill(uvs.len(), "TEXCOORD_0")?;
    let colors: Vec<[f32; 4]> = match reader.read_colors(0) {
        Some(c) => c.into_rgba_f32().collect(),
        None => vec![[1.0; 4]; n],
    };
    fill(colors.len(), "COLOR_0")?;

    let mut mesh = Mesh { positions, normals: vec![[0.0; 3]; n], uvs, colors, indices };
    match reader.read_normals() {
        Some(normals) => {
            mesh.normals = normals.collect();
            fill(mesh.normals.len(), "NORMAL")?;
        }
        // The glTF spec requires flat normals when NORMAL is absent.
        None => mesh.compute_flat_normals(),
    }
    mesh.validate().map_err(|e| e.to_string())?;
    Ok(Some(mesh))
}

fn strip_to_list(strip: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    for i in 2..strip.len() {
        if i % 2 == 0 {
            out.extend_from_slice(&[strip[i - 2], strip[i - 1], strip[i]]);
        } else {
            out.extend_from_slice(&[strip[i - 1], strip[i - 2], strip[i]]);
        }
    }
    out
}

fn fan_to_list(fan: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    for i in 2..fan.len() {
        out.extend_from_slice(&[fan[0], fan[i - 1], fan[i]]);
    }
    out
}

fn convert_image(data: gltf::image::Data, color_space: ColorSpace) -> Result<Image, String> {
    use gltf::image::Format;
    let (w, h) = (data.width, data.height);
    let px = data.pixels;
    let expand = |channels: usize, bytes_per: usize| -> Vec<u8> {
        px.chunks_exact(channels * bytes_per)
            .flat_map(|p| {
                let get = |c: usize| -> u8 {
                    match bytes_per {
                        1 => p[c],
                        2 => p[c * 2 + 1], // little-endian u16: keep the high byte
                        _ => {
                            let b = [p[c * 4], p[c * 4 + 1], p[c * 4 + 2], p[c * 4 + 3]];
                            (f32::from_le_bytes(b).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                        }
                    }
                };
                match channels {
                    1 => [get(0), get(0), get(0), 255],
                    2 => [get(0), get(0), get(0), get(1)],
                    3 => [get(0), get(1), get(2), 255],
                    _ => [get(0), get(1), get(2), get(3)],
                }
            })
            .collect()
    };
    let rgba = match data.format {
        Format::R8 => expand(1, 1),
        Format::R8G8 => expand(2, 1),
        Format::R8G8B8 => expand(3, 1),
        Format::R8G8B8A8 => px,
        Format::R16 => expand(1, 2),
        Format::R16G16 => expand(2, 2),
        Format::R16G16B16 => expand(3, 2),
        Format::R16G16B16A16 => expand(4, 2),
        Format::R32G32B32FLOAT => expand(3, 4),
        Format::R32G32B32A32FLOAT => expand(4, 4),
    };
    let image = Image { width: w, height: h, data: rgba, color_space };
    if image.is_valid() { Ok(image) } else { Err(format!("image data does not match {w}x{h}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_and_fans() {
        assert_eq!(strip_to_list(&[0, 1, 2, 3]), [0, 1, 2, 2, 1, 3]);
        assert_eq!(fan_to_list(&[0, 1, 2, 3]), [0, 1, 2, 0, 2, 3]);
        assert!(strip_to_list(&[0, 1]).is_empty());
    }
}
