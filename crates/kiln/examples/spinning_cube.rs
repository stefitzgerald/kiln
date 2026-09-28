//! TC-MAN-06: a lit cube rotating smoothly on a ground plane; FPS in the title bar.
//!
//! `cargo run --example spinning_cube`

use kiln::prelude::*;

struct Spin {
    radians_per_sec: f32,
}
impl Component for Spin {}

fn main() -> AppExit {
    let mut app = App::new();
    app.add_plugin(DefaultPlugins::titled("Kiln: spinning cube"))
        .add_plugin(ExitOnEscPlugin)
        .add_plugin(FpsTitlePlugin);

    let assets = app.world.init_resource::<AssetServer>();
    let cube = assets.meshes.add(Mesh::cube(1.0));
    let ground = assets.meshes.add(Mesh::plane(8.0));
    let orange = assets.materials.add(Material::color([0.9, 0.35, 0.1, 1.0]));
    let gray = assets
        .materials
        .add(Material::color([0.35, 0.35, 0.38, 1.0]));

    app.world.spawn((
        Transform::from_xyz(0.0, 0.75, 0.0),
        MeshInstance {
            mesh: cube,
            material: orange,
        },
        Spin {
            radians_per_sec: 1.0,
        },
    ));
    app.world.spawn((
        Transform::IDENTITY,
        MeshInstance {
            mesh: ground,
            material: gray,
        },
    ));
    app.world.spawn((
        Transform::from_xyz(3.0, 2.5, 4.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
        Camera::default(),
    ));
    app.world.spawn((
        Transform::IDENTITY.looking_at(Vec3::new(-0.5, -1.0, -0.8), Vec3::Y),
        DirectionalLight {
            intensity: 1.2,
            ..Default::default()
        },
    ));

    app.add_system(Stage::Update, |world: &mut World| {
        let dt = world.resource::<Time>().map_or(0.0, Time::delta_secs);
        for (t, spin) in world.query::<(&mut Transform, &Spin)>() {
            let q = Quat::from_rotation_y(spin.radians_per_sec * dt)
                * Quat::from_rotation_x(0.5 * spin.radians_per_sec * dt);
            t.rotation = (q * t.rotation).normalize();
        }
    });
    app.run()
}
