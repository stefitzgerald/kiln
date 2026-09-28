//! TC-MAN-07/08/11: view a glTF 2.0 file with a fly camera.
//!
//! `cargo run --example gltf_viewer -- path/to/model.glb`
//!
//! Controls: hold right mouse to look, WASD to move, Q/E down/up, Shift faster,
//! scroll to change speed, Esc to quit.

use std::path::PathBuf;

use kiln::math::Aabb;
use kiln::prelude::*;
use kiln::scene::propagate_transforms;

fn main() -> AppExit {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/CheckerCube.glb")
        });

    // Load before opening a window so a bad path fails fast with a clear message.
    let started = std::time::Instant::now();
    let mut assets = AssetServer::new();
    let gltf = match assets.load_gltf(&path) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("usage: cargo run --example gltf_viewer -- <file.gltf|file.glb>");
            return AppExit::error();
        }
    };
    let load_time = started.elapsed();

    let mut app = App::new();
    let title = format!(
        "Kiln: {}",
        path.file_name()
            .map_or("glTF".into(), |n| n.to_string_lossy())
    );
    app.add_plugin(DefaultPlugins::titled(title))
        .add_plugin(ExitOnEscPlugin)
        .add_plugin(FpsTitlePlugin)
        .add_plugin(FlyCameraPlugin);
    tracing::info!(path = %path.display(), ?load_time, "model loaded");
    app.world.insert_resource(assets);

    if let Err(e) = spawn_gltf_scene(&mut app.world, gltf, None) {
        eprintln!("error: {e}");
        return AppExit::error();
    }
    propagate_transforms(&mut app.world);

    // Frame the model: place the camera outside its bounding sphere.
    let bounds = scene_bounds(&app.world).unwrap_or(Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0)));
    let center = bounds.center();
    let radius = bounds.half_extents().length().max(0.01);
    let eye = center + Vec3::new(0.6, 0.4, 1.0).normalize() * radius * 2.5;
    let transform = Transform::from_translation(eye).looking_at(center, Vec3::Y);
    app.world.spawn((
        transform,
        Camera {
            projection: Projection::Perspective {
                fov_y: 50f32.to_radians(),
                near: radius * 0.01,
                far: radius * 100.0,
            },
            ..Default::default()
        },
        FlyCamera::from_rotation(transform.rotation, radius),
    ));
    app.world.spawn((
        Transform::IDENTITY.looking_at(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
        DirectionalLight {
            intensity: 1.5,
            ..Default::default()
        },
    ));
    app.insert_resource(AmbientLight {
        color: [1.0; 3],
        intensity: 0.25,
    });
    app.run()
}

fn scene_bounds(world: &World) -> Option<Aabb> {
    let assets = world.resource::<AssetServer>()?;
    world
        .query_ref::<(&MeshInstance, &GlobalTransform), ()>()
        .filter_map(|(m, g)| {
            let aabb = assets.meshes.get(m.mesh)?.aabb()?;
            Some(aabb.transformed(&g.0))
        })
        .reduce(|a, b| a.union(&b))
}
