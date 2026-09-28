//! TC-MAN-05: an RGB-gradient triangle, centered, red corner at the top.
//!
//! `cargo run --example triangle`

use kiln::prelude::*;

fn main() -> AppExit {
    let mut app = App::new();
    app.add_plugin(DefaultPlugins::titled("Kiln: triangle"))
        .add_plugin(ExitOnEscPlugin);
    app.insert_resource(ClearColor([0.0, 0.0, 0.0, 1.0]));

    let assets = app.world.init_resource::<AssetServer>();
    let mesh = assets.meshes.add(Mesh::triangle());
    let material = assets.materials.add(Material::unlit([1.0; 4]));

    app.world
        .spawn((Transform::IDENTITY, MeshInstance { mesh, material }));
    app.world.spawn((
        Transform::from_xyz(0.0, 0.0, 1.0),
        Camera {
            projection: Projection::Orthographic {
                height: 1.5,
                near: 0.1,
                far: 10.0,
            },
            ..Default::default()
        },
    ));
    app.run()
}
