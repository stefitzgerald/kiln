//! Renderer GPU tests (TC-GPU-02..08, TC-AST-08). Ignored by default; run with
//! `cargo xtask gpu-test`. Re-render golden images with `cargo xtask bless-goldens`.
//!
//! Golden comparison: a pixel "differs" if any channel is off by more than 2; a test fails
//! if more than 0.5% of pixels differ. On failure `target/golden-diff/<name>_{actual,diff}.png`
//! are written for inspection.

#![allow(clippy::unwrap_used, clippy::chunks_exact_to_as_chunks)]

use std::path::PathBuf;

use kiln_asset::{AssetServer, Handle, Image, Material, Mesh};
use kiln_math::{Mat4, Quat, Vec3, look_at, perspective_reverse_z};
use kiln_render::{
    DirectionalLightData, DrawItem, FrameStatus, RenderScene, Renderer, RendererSettings,
    Validation,
};

const SIZE: u32 = 256;
const GPU: &str = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn renderer() -> Renderer {
    let validation = if std::env::var_os("CI").is_some() {
        Validation::Required
    } else {
        Validation::Enabled
    };
    let settings = RendererSettings {
        validation,
        ..Default::default()
    };
    Renderer::new_headless(SIZE, SIZE, &settings).expect("headless renderer")
}

/// TC-GPU-08: every GPU test ends with zero validation errors and warnings.
fn assert_clean(r: &Renderer) {
    let stats = r.validation_stats();
    assert_eq!(
        (stats.errors, stats.warnings),
        (0, 0),
        "validation messages: {stats:?}"
    );
}

fn render(r: &mut Renderer, scene: &RenderScene, assets: &AssetServer) -> Image {
    assert_eq!(r.render(scene, assets).unwrap(), FrameStatus::Rendered);
    r.read_pixels().unwrap()
}

fn camera(eye: Vec3, target: Vec3) -> (Mat4, Vec3) {
    (
        perspective_reverse_z(45f32.to_radians(), 1.0, 0.1, 100.0) * look_at(eye, target, Vec3::Y),
        eye,
    )
}

fn luminance(p: [u8; 4]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn linear_to_srgb8(c: f32) -> u8 {
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// Compare `actual` against `tests/assets/goldens/<name>.png` (or write it when blessing).
fn check_golden(name: &str, actual: &Image) {
    let path = root()
        .join("tests/assets/goldens")
        .join(format!("{name}.png"));
    let to_png =
        |img: &Image| image::RgbaImage::from_raw(img.width, img.height, img.data.clone()).unwrap();
    if std::env::var_os("KILN_BLESS").is_some() {
        to_png(actual).save(&path).unwrap();
        eprintln!("blessed {}", path.display());
        return;
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| {
            panic!(
                "missing golden {} ({e}); run `cargo xtask bless-goldens`",
                path.display()
            )
        })
        .to_rgba8();
    assert_eq!(
        (expected.width(), expected.height()),
        (actual.width, actual.height),
        "golden size mismatch"
    );
    let mut diff = image::RgbaImage::new(actual.width, actual.height);
    let mut bad = 0usize;
    for (i, (e, a)) in expected
        .as_raw()
        .chunks_exact(4)
        .zip(actual.data.chunks_exact(4))
        .enumerate()
    {
        let d = e
            .iter()
            .zip(a)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        let (x, y) = (i as u32 % actual.width, i as u32 / actual.width);
        if d > 2 {
            bad += 1;
            diff.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        } else {
            diff.put_pixel(x, y, image::Rgba([a[0] / 4, a[1] / 4, a[2] / 4, 255]));
        }
    }
    let total = (actual.width * actual.height) as usize;
    if bad * 1000 > total * 5 {
        let dir = root().join("target/golden-diff");
        std::fs::create_dir_all(&dir).unwrap();
        to_png(actual)
            .save(dir.join(format!("{name}_actual.png")))
            .unwrap();
        diff.save(dir.join(format!("{name}_diff.png"))).unwrap();
        panic!(
            "golden `{name}` mismatch: {bad}/{total} pixels differ (> 0.5%); see {}",
            dir.display()
        );
    }
}

/// TC-GPU-02: clear color round-trips through the sRGB target exactly.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_02_clear_color_readback() {
    let _ = GPU;
    let mut r = renderer();
    let scene = RenderScene {
        clear_color: [0.2, 0.4, 0.6, 1.0],
        ..Default::default()
    };
    let img = render(&mut r, &scene, &AssetServer::new());
    let expected = [
        linear_to_srgb8(0.2),
        linear_to_srgb8(0.4),
        linear_to_srgb8(0.6),
        255,
    ];
    for px in img.data.chunks_exact(4) {
        for c in 0..4 {
            assert!(
                px[c].abs_diff(expected[c]) <= 1,
                "pixel {px:?} != {expected:?}"
            );
        }
    }
    assert_clean(&r);
}

/// TC-GPU-03: the RGB triangle, and Y is up (the red vertex is at the top).
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_03_triangle() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let mesh = assets.meshes.add(Mesh::triangle());
    let material = assets.materials.add(Material::unlit([1.0; 4]));
    let scene = RenderScene {
        clear_color: [0.0, 0.0, 0.0, 1.0],
        draws: vec![DrawItem {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        }],
        ..Default::default()
    };
    let img = render(&mut r, &scene, &assets);
    // Corners are background.
    assert_eq!(img.pixel(0, 0), [0, 0, 0, 255]);
    assert_eq!(img.pixel(SIZE - 1, SIZE - 1), [0, 0, 0, 255]);
    // Near the top vertex (NDC y = 0.5 → row 64) red dominates.
    let top = img.pixel(SIZE / 2, SIZE / 4 + 6);
    assert!(
        top[0] > top[1] && top[0] > top[2],
        "top should be red, got {top:?}"
    );
    // Bottom-left is green, bottom-right is blue.
    let bl = img.pixel(SIZE / 4 + 8, SIZE * 3 / 4 - 4);
    let br = img.pixel(SIZE * 3 / 4 - 8, SIZE * 3 / 4 - 4);
    assert!(
        bl[1] > bl[0] && bl[1] > bl[2],
        "bottom-left should be green, got {bl:?}"
    );
    assert!(
        br[2] > br[0] && br[2] > br[1],
        "bottom-right should be blue, got {br:?}"
    );
    check_golden("triangle", &img);
    assert_clean(&r);
}

fn lit_scene(draws: Vec<DrawItem>, eye: Vec3) -> RenderScene {
    let (view_projection, camera_position) = camera(eye, Vec3::ZERO);
    RenderScene {
        view_projection,
        camera_position,
        clear_color: [0.02, 0.02, 0.03, 1.0],
        ambient: [0.1; 3],
        light: Some(DirectionalLightData {
            direction: Vec3::new(-1.0, -2.0, -1.5),
            color: [1.0; 3],
        }),
        draws,
    }
}

/// TC-GPU-04: lit cube with depth; back faces are culled and front faces lit.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_04_cube_depth() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let mesh = assets.meshes.add(Mesh::cube(1.0));
    let material = assets.materials.add(Material::color([0.8, 0.3, 0.2, 1.0]));
    let scene = lit_scene(
        vec![DrawItem {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        }],
        Vec3::new(2.0, 2.0, 2.0),
    );
    let img = render(&mut r, &scene, &assets);
    // The top face (+Y) faces the light; if winding/culling were wrong we would see the
    // inside of the cube, lit by ambient only.
    let top_face = img.pixel(SIZE / 2, SIZE / 2 - 30);
    let ambient_only = linear_to_srgb8(0.8 * 0.1);
    assert!(
        top_face[0] > ambient_only + 40,
        "top face not lit: {top_face:?}"
    );
    assert_eq!(r.stats().draws, 1);
    check_golden("cube_depth", &img);
    assert_clean(&r);
}

/// TC-GPU-05: overlapping quads resolve by depth, not draw order (reverse-Z works).
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_05_depth_order_independent() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let quad = assets.meshes.add(Mesh::quad(2.0));
    let red = assets.materials.add(Material::unlit([1.0, 0.0, 0.0, 1.0]));
    let green = assets.materials.add(Material::unlit([0.0, 1.0, 0.0, 1.0]));
    let near = DrawItem {
        mesh: quad,
        material: red,
        transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0)),
    };
    let far = DrawItem {
        mesh: quad,
        material: green,
        transform: Mat4::from_translation(Vec3::new(1.5, 1.5, -3.0)),
    };
    let (view_projection, camera_position) =
        camera(Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 0.0, -1.0));
    let base = RenderScene {
        view_projection,
        camera_position,
        ..Default::default()
    };

    let a = render(
        &mut r,
        &RenderScene {
            draws: vec![far, near],
            ..base.clone()
        },
        &assets,
    );
    let b = render(
        &mut r,
        &RenderScene {
            draws: vec![near, far],
            ..base
        },
        &assets,
    );
    assert_eq!(a.data, b.data, "draw order changed the image");
    assert_eq!(
        a.pixel(SIZE / 2, SIZE / 2),
        [255, 0, 0, 255],
        "near quad must win"
    );
    assert!(
        a.data.chunks_exact(4).any(|p| p == [0, 255, 0, 255]),
        "far quad visible where uncovered"
    );
    assert_clean(&r);
}

/// TC-GPU-06: a directional light makes the lit side of a sphere brighter.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_06_directional_light() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let mesh = assets.meshes.add(Mesh::uv_sphere(1.0, 48, 24));
    let material = assets.materials.add(Material::color([0.9, 0.9, 0.9, 1.0]));
    let (view_projection, camera_position) = camera(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO);
    let scene = RenderScene {
        view_projection,
        camera_position,
        clear_color: [0.0, 0.0, 0.0, 1.0],
        ambient: [0.05; 3],
        // Light travels toward −X, so the +X side (right of the image) is lit.
        light: Some(DirectionalLightData {
            direction: Vec3::NEG_X,
            color: [1.0; 3],
        }),
        draws: vec![DrawItem {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        }],
    };
    let img = render(&mut r, &scene, &assets);
    let right = luminance(img.pixel(SIZE / 2 + 50, SIZE / 2));
    let left = luminance(img.pixel(SIZE / 2 - 50, SIZE / 2));
    assert!(
        right > left + 60.0,
        "lit side {right} vs shadow side {left}"
    );
    assert!(left > 0.0, "ambient keeps the dark side visible");
    check_golden("sphere_lit", &img);
    assert_clean(&r);
}

/// TC-GPU-07: sustained rendering does not leak GPU memory or produce validation messages.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_07_many_frames_stable_memory() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let mesh = assets.meshes.add(Mesh::cube(0.3));
    let material = assets.materials.add(Material::color([0.2, 0.6, 0.9, 1.0]));
    let mut baseline = None;
    for frame in 0..300u32 {
        let angle = frame as f32 * 0.02;
        let draws = (0..50)
            .map(|i| {
                let t = Mat4::from_rotation_translation(
                    Quat::from_rotation_y(angle + i as f32),
                    Vec3::new((i % 10) as f32 - 4.5, (i / 10) as f32 - 2.0, -8.0),
                );
                DrawItem {
                    mesh,
                    material,
                    transform: t,
                }
            })
            .collect();
        let scene = lit_scene(draws, Vec3::new(0.0, 0.0, 4.0));
        assert_eq!(r.render(&scene, &assets).unwrap(), FrameStatus::Rendered);
        if frame == 10 {
            baseline = Some(r.memory_report());
        }
    }
    r.read_pixels().unwrap();
    let end = r.memory_report();
    assert_eq!(
        Some(end),
        baseline,
        "GPU memory changed during steady-state rendering"
    );
    assert_clean(&r);
}

/// TC-AST-08: the textured glTF cube renders with its checker texture.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_ast_08_textured_gltf() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let gltf = assets
        .load_gltf(root().join("tests/assets/CheckerCube.glb"))
        .unwrap();
    let prim = assets.gltfs.get(gltf).unwrap().meshes[0].primitives[0];
    let transform = Mat4::from_rotation_y(0.5);
    let mut scene = lit_scene(
        vec![DrawItem {
            mesh: prim.mesh,
            material: prim.material,
            transform,
        }],
        Vec3::new(1.6, 1.4, 2.2),
    );
    scene.ambient = [0.4; 3];
    let img = render(&mut r, &scene, &assets);
    // Both checker colors (orange and dark gray) appear on the cube.
    let center_region: Vec<[u8; 4]> = (96..160)
        .flat_map(|y| (96..160).map(move |x| (x, y)))
        .map(|(x, y)| img.pixel(x, y))
        .collect();
    assert!(
        center_region.iter().any(|p| p[0] > p[2] + 60),
        "orange squares missing"
    );
    assert!(
        center_region
            .iter()
            .any(|p| p[0].abs_diff(p[2]) < 20 && p[0] < 90),
        "gray squares missing"
    );
    check_golden("checker_cube", &img);
    assert_clean(&r);
}

#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn frustum_culling_and_missing_assets() {
    let mut r = renderer();
    let mut assets = AssetServer::new();
    let mesh = assets.meshes.add(Mesh::cube(1.0));
    let material = assets.materials.add(Material::default());
    let behind = DrawItem {
        mesh,
        material,
        transform: Mat4::from_translation(Vec3::new(0.0, 0.0, 50.0)),
    };
    let visible = DrawItem {
        mesh,
        material,
        transform: Mat4::IDENTITY,
    };
    let missing_mesh = DrawItem {
        mesh: Handle::from_raw_parts(99, 0),
        material,
        transform: Mat4::IDENTITY,
    };
    let missing_material = DrawItem {
        mesh,
        material: Handle::from_raw_parts(99, 0),
        transform: Mat4::IDENTITY,
    };
    let scene = lit_scene(
        vec![behind, visible, missing_mesh, missing_material],
        Vec3::new(0.0, 0.0, 5.0),
    );
    render(&mut r, &scene, &assets);
    let stats = r.stats();
    assert_eq!(stats.culled, 1);
    assert_eq!(stats.draws, 2, "missing material falls back to the default");
    assert_clean(&r);
}

#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn headless_resize() {
    let mut r = renderer();
    r.resize(128, 64).unwrap();
    let img = render(
        &mut r,
        &RenderScene {
            clear_color: [1.0; 4],
            ..Default::default()
        },
        &AssetServer::new(),
    );
    assert_eq!((img.width, img.height), (128, 64));
    assert!(img.data.iter().all(|&b| b == 255));
    r.resize(0, 0).unwrap(); // ignored for headless targets
    assert_eq!(r.extent().width, 128);
    assert_clean(&r);
}
