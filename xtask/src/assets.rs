//! Test asset generation and download.

use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use kiln_asset::{Image, Mesh};

/// Generate `tests/assets/CheckerCube.glb`: a unit cube with a 64×64 checker texture.
pub(crate) fn generate(root: &Path) -> Result<(), String> {
    let mesh = Mesh::cube(1.0);
    let checker = Image::checkerboard(64, 8, [230, 120, 40, 255], [40, 40, 48, 255]);
    let mut png = Vec::new();
    image::RgbaImage::from_raw(checker.width, checker.height, checker.data)
        .ok_or("bad checker image")?
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut push = |bytes: &[u8]| {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        views.push((offset, bytes.len()));
    };
    let f32s = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
    push(&f32s(mesh.positions.as_flattened()));
    push(&f32s(mesh.normals.as_flattened()));
    push(&f32s(mesh.uvs.as_flattened()));
    push(
        &mesh
            .indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    push(&png);

    let aabb = mesh.aabb().ok_or("empty mesh")?;
    let view_json = views
        .iter()
        .enumerate()
        .map(|(i, (offset, len))| {
            let target = match i {
                0..=2 => r#", "target": 34962"#,
                3 => r#", "target": 34963"#,
                _ => "",
            };
            format!(r#"{{"buffer": 0, "byteOffset": {offset}, "byteLength": {len}{target}}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let n = mesh.vertex_count();
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0", "generator": "kiln xtask gen-assets"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"name": "CheckerCube", "mesh": 0}}],
  "meshes": [{{"name": "CheckerCube", "primitives": [{{
    "attributes": {{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}},
    "indices": 3, "material": 0}}]}}],
  "materials": [{{"name": "Checker", "pbrMetallicRoughness": {{
    "baseColorTexture": {{"index": 0}}, "metallicFactor": 0.0, "roughnessFactor": 1.0}}}}],
  "textures": [{{"sampler": 0, "source": 0}}],
  "samplers": [{{"magFilter": 9728, "minFilter": 9986}}],
  "images": [{{"bufferView": 4, "mimeType": "image/png"}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": {n}, "type": "VEC3",
      "min": [{}, {}, {}], "max": [{}, {}, {}]}},
    {{"bufferView": 1, "componentType": 5126, "count": {n}, "type": "VEC3"}},
    {{"bufferView": 2, "componentType": 5126, "count": {n}, "type": "VEC2"}},
    {{"bufferView": 3, "componentType": 5125, "count": {}, "type": "SCALAR"}}
  ],
  "bufferViews": [{view_json}],
  "buffers": [{{"byteLength": {}}}]
}}"#,
        aabb.min.x,
        aabb.min.y,
        aabb.min.z,
        aabb.max.x,
        aabb.max.y,
        aabb.max.z,
        mesh.indices.len(),
        bin.len()
    );

    let glb = make_glb(&json, &bin);
    let out = root.join("tests/assets/CheckerCube.glb");
    std::fs::write(&out, glb).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}

fn make_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

const EXTERNAL: &[(&str, &str)] = &[(
    "DamagedHelmet.glb",
    "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
)];

/// Download sample models that are too large to commit.
pub(crate) fn fetch(root: &Path) -> Result<(), String> {
    let dir = root.join("assets/external");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, url) in EXTERNAL {
        let dest = dir.join(name);
        if dest.exists() {
            println!("{name}: already present");
            continue;
        }
        println!("{name}: downloading {url}");
        let status = Command::new("curl")
            .args(["-sSfL", "-o"])
            .arg(&dest)
            .arg(url)
            .status()
            .map_err(|e| format!("curl not available: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&dest);
            return Err(format!("download of {name} failed"));
        }
    }
    println!(
        "Sample models are in {} (see the model licenses at the source URLs).",
        dir.display()
    );
    Ok(())
}
