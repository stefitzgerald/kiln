//! Scene acceptance tests (TC-SCN-*, TC-AST-07). See docs/testing/M0-test-plan.md.

use kiln_app::{App, Stage};
use kiln_asset::AssetServer;
use kiln_ecs::{Entity, World};
use kiln_math::{Affine3A, Mat4, Quat, Vec3};
use kiln_scene::*;

fn global(world: &World, e: Entity) -> Affine3A {
    world.get::<GlobalTransform>(e).expect("GlobalTransform").0
}

fn pos(world: &World, e: Entity) -> Vec3 {
    global(world, e).translation.into()
}

/// TC-SCN-01
#[test]
fn tc_scn_01_child_inherits_parent_translation() {
    let mut w = World::new();
    let parent = w.spawn(Transform::from_xyz(1.0, 0.0, 0.0));
    let child = w.spawn(Transform::from_xyz(0.0, 1.0, 0.0));
    set_parent(&mut w, child, parent).unwrap();
    propagate_transforms(&mut w);
    assert!(pos(&w, child).abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 1e-6));
    assert!(pos(&w, parent).abs_diff_eq(Vec3::X, 1e-6));
}

/// TC-SCN-02
#[test]
fn tc_scn_02_rotation_and_scale_compose() {
    let mut w = World::new();
    let parent_tf = Transform::from_xyz(0.0, 0.0, -3.0)
        .with_rotation(Quat::from_rotation_y(90f32.to_radians()))
        .with_scale(Vec3::splat(2.0));
    let parent = w.spawn(parent_tf);
    let child = w.spawn(Transform::from_xyz(1.0, 0.0, 0.0));
    set_parent(&mut w, child, parent).unwrap();
    propagate_transforms(&mut w);

    // Hand computation: scale (1,0,0) by 2 → (2,0,0); rotate +90° about Y → (0,0,-2);
    // translate by (0,0,-3) → (0,0,-5).
    assert!(pos(&w, child).abs_diff_eq(Vec3::new(0.0, 0.0, -5.0), 1e-5), "{}", pos(&w, child));
    let expected = parent_tf.to_matrix() * Mat4::from_translation(Vec3::X);
    assert!(Mat4::from(global(&w, child)).abs_diff_eq(expected, 1e-5));
}

/// TC-SCN-03
#[test]
fn tc_scn_03_reparent_updates_both_sides() {
    let mut w = World::new();
    let a = w.spawn(Transform::from_xyz(10.0, 0.0, 0.0));
    let b = w.spawn(Transform::from_xyz(-10.0, 0.0, 0.0));
    let child = w.spawn(Transform::from_xyz(0.0, 1.0, 0.0));
    set_parent(&mut w, child, a).unwrap();
    propagate_transforms(&mut w);
    assert!(pos(&w, child).abs_diff_eq(Vec3::new(10.0, 1.0, 0.0), 1e-6));

    set_parent(&mut w, child, b).unwrap();
    propagate_transforms(&mut w);
    assert!(pos(&w, child).abs_diff_eq(Vec3::new(-10.0, 1.0, 0.0), 1e-6));
    assert_eq!(w.get::<Parent>(child), Some(&Parent(b)));
    assert!(w.get::<Children>(a).is_none(), "old parent's Children cleaned up");
    assert_eq!(w.get::<Children>(b), Some(&Children(vec![child])));

    remove_parent(&mut w, child);
    propagate_transforms(&mut w);
    assert!(w.get::<Parent>(child).is_none());
    assert!(pos(&w, child).abs_diff_eq(Vec3::Y, 1e-6), "now a root");
}

/// TC-SCN-04
#[test]
fn tc_scn_04_cycles_are_rejected() {
    let mut w = World::new();
    let a = w.spawn(Transform::IDENTITY);
    let b = w.spawn(Transform::IDENTITY);
    let c = w.spawn(Transform::IDENTITY);
    set_parent(&mut w, b, a).unwrap();
    set_parent(&mut w, c, b).unwrap();

    assert_eq!(set_parent(&mut w, a, c), Err(HierarchyError::Cycle { child: a, parent: c }));
    assert_eq!(set_parent(&mut w, a, a), Err(HierarchyError::Cycle { child: a, parent: a }));
    // Unchanged.
    assert!(w.get::<Parent>(a).is_none());
    assert_eq!(w.get::<Children>(c), None);
    assert_eq!(w.get::<Parent>(c), Some(&Parent(b)));

    let dead = w.spawn(Transform::IDENTITY);
    w.despawn(dead);
    assert_eq!(set_parent(&mut w, a, dead), Err(HierarchyError::NoSuchEntity(dead)));
}

/// TC-SCN-05
#[test]
fn tc_scn_05_recursive_despawn() {
    let mut w = World::new();
    let keep = w.spawn(Transform::IDENTITY);
    let root = w.spawn(Transform::IDENTITY);
    set_parent(&mut w, root, keep).unwrap();
    let mut all = vec![root];
    for _ in 0..3 {
        let c = w.spawn(Transform::IDENTITY);
        set_parent(&mut w, c, root).unwrap();
        let gc = w.spawn(Transform::IDENTITY);
        set_parent(&mut w, gc, c).unwrap();
        all.extend([c, gc]);
    }
    assert_eq!(despawn_recursive(&mut w, root), 7);
    assert!(all.iter().all(|e| !w.is_alive(*e)));
    assert!(w.is_alive(keep));
    assert!(w.get::<Children>(keep).is_none(), "detached from surviving parent");
    assert_eq!(w.entity_count(), 1);
}

/// TC-SCN-06
#[test]
fn tc_scn_06_deep_chain() {
    let mut w = World::new();
    let mut prev = w.spawn(Transform::from_xyz(1.0, 0.0, 0.0));
    let first = prev;
    let mut chain = vec![prev];
    for _ in 0..3 {
        let e = w.spawn(Transform::from_xyz(1.0, 0.0, 0.0));
        set_parent(&mut w, e, prev).unwrap();
        chain.push(e);
        prev = e;
    }
    propagate_transforms(&mut w);
    for (depth, e) in chain.iter().enumerate() {
        assert!(pos(&w, *e).abs_diff_eq(Vec3::X * (depth as f32 + 1.0), 1e-6));
    }
    // Moving the root moves every descendant on the next propagation.
    w.get_mut::<Transform>(first).unwrap().translation = Vec3::new(0.0, 5.0, 0.0);
    propagate_transforms(&mut w);
    assert!(pos(&w, prev).abs_diff_eq(Vec3::new(3.0, 5.0, 0.0), 1e-6));
}

#[test]
fn orphan_of_despawned_parent_becomes_root() {
    let mut w = World::new();
    let p = w.spawn(Transform::from_xyz(5.0, 0.0, 0.0));
    let c = w.spawn(Transform::from_xyz(1.0, 0.0, 0.0));
    set_parent(&mut w, c, p).unwrap();
    w.despawn(p); // non-recursive: leaves a dangling Parent
    propagate_transforms(&mut w);
    assert!(pos(&w, c).abs_diff_eq(Vec3::X, 1e-6));
}

#[test]
fn scene_plugin_propagates_every_frame() {
    let mut app = App::new();
    app.add_plugin(ScenePlugin);
    let e = app.world.spawn(Transform::from_xyz(0.0, 0.0, 0.0));
    app.add_system(Stage::Update, move |w| {
        w.get_mut::<Transform>(e).unwrap().translation.x += 1.0;
    });
    app.update_with_delta(std::time::Duration::from_millis(16));
    app.update_with_delta(std::time::Duration::from_millis(16));
    assert!(pos(&app.world, e).abs_diff_eq(Vec3::X * 2.0, 1e-6));
}

#[test]
fn camera_projection() {
    let cam = Camera::default();
    let at = GlobalTransform(Transform::from_xyz(0.0, 0.0, 5.0).to_affine());
    let vp = cam.view_projection(&at, 16.0 / 9.0);
    let clip = vp * Vec3::ZERO.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    assert!(ndc.x.abs() < 1e-6 && ndc.y.abs() < 1e-6, "origin is centered");
    assert!(ndc.z > 0.0 && ndc.z < 1.0);
}

/// TC-AST-07: spawning a glTF scene mirrors its node tree in the ECS.
#[test]
fn tc_ast_07_spawn_gltf_scene_hierarchy() {
    let mut w = World::new();
    let mut server = AssetServer::new();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/Box.glb");
    let handle = server.load_gltf(path).unwrap();
    let node_count = server.gltfs.get(handle).unwrap().nodes.len();
    w.insert_resource(server);

    let root = spawn_gltf_scene(&mut w, handle, None).unwrap();
    propagate_transforms(&mut w);

    // Box.glb: root node (with a -90° X rotation) → child node carrying the mesh.
    let root_children = &w.get::<Children>(root).unwrap().0;
    assert_eq!(root_children.len(), 1);
    let node0 = root_children[0];
    let node1 = w.get::<Children>(node0).unwrap().0[0];
    assert_eq!(w.entity_count(), 1 + node_count);
    assert!(w.get::<MeshInstance>(node1).is_some());
    assert!(w.get::<MeshInstance>(node0).is_none());
    assert_eq!(w.get::<Parent>(node1), Some(&Parent(node0)));

    // Global of the mesh node equals the product of the local transforms.
    let expected = Mat4::from(global(&w, node0)) * w.get::<Transform>(node1).unwrap().to_matrix();
    assert!(Mat4::from(global(&w, node1)).abs_diff_eq(expected, 1e-5));

    assert_eq!(despawn_recursive(&mut w, root), 1 + node_count);
    assert_eq!(w.entity_count(), 0);
}

#[test]
fn spawn_errors() {
    let mut w = World::new();
    let bogus = kiln_asset::Handle::from_raw_parts(0, 0);
    assert_eq!(spawn_gltf_scene(&mut w, bogus, None), Err(SceneError::NoAssetServer));
    w.insert_resource(AssetServer::new());
    assert_eq!(spawn_gltf_scene(&mut w, bogus, None), Err(SceneError::NotLoaded(bogus)));
}
