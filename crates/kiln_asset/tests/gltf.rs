//! glTF import acceptance tests (TC-AST-*). See docs/testing/M0-test-plan.md.

use std::path::PathBuf;

use kiln_asset::{AssetError, AssetServer, ColorSpace};
use kiln_math::Vec3;

fn asset_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets").join(name)
}

/// Build a GLB container from a JSON document and a binary chunk.
fn make_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// A single triangle without normals, placed under a three-level node hierarchy.
fn triangle_hierarchy_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let bin: Vec<u8> = positions.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
    let json = format!(
        r#"{{
        "asset": {{"version": "2.0"}},
        "scene": 0,
        "scenes": [{{"name": "Main", "nodes": [0]}}],
        "nodes": [
            {{"name": "root", "translation": [1, 0, 0], "children": [1]}},
            {{"name": "middle", "translation": [0, 1, 0], "children": [2]}},
            {{"name": "leaf", "translation": [0, 0, 1], "mesh": 0}}
        ],
        "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}}}]}}],
        "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                        "min": [0, 0, 0], "max": [1, 1, 0]}}],
        "bufferViews": [{{"buffer": 0, "byteLength": {len}}}],
        "buffers": [{{"byteLength": {len}}}]
    }}"#,
        len = bin.len()
    );
    make_glb(&json, &bin)
}

/// TC-AST-01
#[test]
fn tc_ast_01_box_glb() {
    let mut server = AssetServer::new();
    let h = server.load_gltf(asset_path("Box.glb")).unwrap();
    let gltf = server.gltfs.get(h).unwrap();
    assert_eq!(gltf.meshes.len(), 1);
    assert_eq!(gltf.meshes[0].primitives.len(), 1);
    let mesh = server.meshes.get(gltf.meshes[0].primitives[0].mesh).unwrap();
    assert_eq!(mesh.vertex_count(), 24);
    assert_eq!(mesh.indices.len(), 36);
    let aabb = mesh.aabb().unwrap();
    assert!(aabb.min.abs_diff_eq(Vec3::splat(-0.5), 1e-6), "{aabb:?}");
    assert!(aabb.max.abs_diff_eq(Vec3::splat(0.5), 1e-6), "{aabb:?}");
    // Box.glb has a red-ish material and a two-node hierarchy.
    let mat = server.materials.get(gltf.meshes[0].primitives[0].material).unwrap();
    assert!(mat.base_color[0] > 0.5 && mat.base_color[1] < 0.5);
    assert_eq!(gltf.default_roots().len(), 1);
}

/// TC-AST-02
#[test]
fn tc_ast_02_same_path_is_loaded_once() {
    let mut server = AssetServer::new();
    let a = server.load_gltf(asset_path("Box.glb")).unwrap();
    // A different spelling of the same file must hit the cache too.
    let b = server.load_gltf(asset_path("../assets/Box.glb")).unwrap();
    assert_eq!(a, b);
    assert_eq!(server.load_count(), 1);
    assert_eq!(server.meshes.len(), 1);
}

/// TC-AST-03
#[test]
fn tc_ast_03_missing_file_is_not_found() {
    let mut server = AssetServer::new();
    let err = server.load_gltf("does/not/exist.glb").unwrap_err();
    assert!(matches!(&err, AssetError::NotFound { path } if path.ends_with("exist.glb")));
    assert!(err.to_string().contains("does/not/exist.glb"), "{err}");
}

/// TC-AST-04: every truncation and a set of byte corruptions produce errors, never panics.
#[test]
fn tc_ast_04_corrupt_files_error_without_panicking() {
    let good = std::fs::read(asset_path("Box.glb")).unwrap();
    let mut server = AssetServer::new();
    for cut in (0..good.len()).step_by(7) {
        let result = server.load_gltf_from_bytes(&good[..cut], None);
        assert!(matches!(result, Err(AssetError::Parse { .. })), "truncated at {cut} accepted");
    }
    // Deterministic pseudo-random byte flips.
    let mut seed = 0x2545_f491_u32;
    let mut accepted = 0;
    for _ in 0..300 {
        let mut bad = good.clone();
        for _ in 0..4 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let i = seed as usize % bad.len();
            bad[i] = (seed >> 8) as u8;
        }
        // Some flips land in padding or float data and are legitimately still valid files;
        // the requirement is only "no panic, and errors are Parse errors".
        match server.load_gltf_from_bytes(&bad, None) {
            Ok(_) => accepted += 1,
            Err(AssetError::Parse { .. }) => {}
            Err(e) => panic!("unexpected error kind: {e}"),
        }
    }
    assert!(accepted < 300, "corruption was never detected");
    let err = server.load_gltf_from_bytes(b"not a gltf", None).unwrap_err();
    assert!(err.to_string().contains("<memory>"));
}

/// TC-AST-05
#[test]
fn tc_ast_05_missing_normals_are_generated_flat() {
    let mut server = AssetServer::new();
    let h = server.load_gltf_from_bytes(&triangle_hierarchy_glb(), None).unwrap();
    let gltf = server.gltfs.get(h).unwrap();
    let prim = gltf.meshes[0].primitives[0];
    let mesh = server.meshes.get(prim.mesh).unwrap();
    assert_eq!(mesh.normals.len(), 3);
    for n in &mesh.normals {
        let n = Vec3::from(*n);
        assert!((n.length() - 1.0).abs() < 1e-6);
        assert!(n.abs_diff_eq(Vec3::Z, 1e-6), "CCW triangle in XY faces +Z, got {n}");
    }
    // No material in the file => shared default material.
    let mat = server.materials.get(prim.material).unwrap();
    assert_eq!(mat.base_color, [1.0; 4]);
}

/// TC-AST-06
#[test]
fn tc_ast_06_textured_cube() {
    let mut server = AssetServer::new();
    let h = server
        .load_gltf(asset_path("CheckerCube.glb"))
        .expect("run `cargo xtask gen-assets` to create CheckerCube.glb");
    let gltf = server.gltfs.get(h).unwrap();
    let prim = gltf.meshes[0].primitives[0];
    let mat = server.materials.get(prim.material).unwrap();
    let tex = mat.base_color_texture.expect("base color texture");
    let image = server.images.get(tex).unwrap();
    assert!(image.is_valid());
    assert_eq!((image.width, image.height), (64, 64));
    assert_eq!(image.color_space, ColorSpace::Srgb);
    // Two distinct checker colors.
    assert_ne!(image.pixel(0, 0), image.pixel(image.width - 1, 0));
    let mesh = server.meshes.get(prim.mesh).unwrap();
    assert!(mesh.uvs.iter().any(|uv| uv[0] > 0.5), "UVs present");
}

/// TC-AST-07 (structure): node hierarchy and transforms are preserved.
#[test]
fn tc_ast_07_node_hierarchy_is_preserved() {
    let mut server = AssetServer::new();
    let h = server.load_gltf_from_bytes(&triangle_hierarchy_glb(), None).unwrap();
    let gltf = server.gltfs.get(h).unwrap();
    assert_eq!(gltf.default_roots(), [0]);
    assert_eq!(gltf.nodes[0].children, [1]);
    assert_eq!(gltf.nodes[1].children, [2]);
    assert_eq!(gltf.nodes[2].mesh, Some(0));
    assert_eq!(gltf.nodes[2].name.as_deref(), Some("leaf"));
    assert_eq!(gltf.nodes[1].transform.translation, Vec3::Y);
    assert_eq!(gltf.scenes[0].name.as_deref(), Some("Main"));
}
