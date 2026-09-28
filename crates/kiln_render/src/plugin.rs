//! ECS integration: extract the scene from the world and render it every frame.

use kiln_app::{App, AppExit, Plugin, Stage, request_exit};
use kiln_asset::AssetServer;
use kiln_ecs::World;
use kiln_math::{Mat4, Vec3};
use kiln_platform::{PrimaryWindow, WindowSize};
use kiln_scene::{
    AmbientLight, Camera, ClearColor, DirectionalLight, GlobalTransform, MeshInstance,
};

use crate::{DirectionalLightData, DrawItem, RenderScene, Renderer, RendererSettings};

/// Creates a [`Renderer`] for the [`PrimaryWindow`] and draws the ECS scene every frame in
/// [`Stage::Render`].
///
/// The scene is: the first active [`Camera`], the first [`DirectionalLight`], the
/// [`AmbientLight`] and [`ClearColor`] resources, and every entity with a [`MeshInstance`]
/// and [`GlobalTransform`].
#[derive(Debug, Default)]
pub struct RenderPlugin {
    /// Renderer configuration.
    pub settings: RendererSettings,
}

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .add_system(Stage::Render, render_system);
    }
}

/// Build a [`RenderScene`] from the world for a target with the given aspect ratio.
pub fn extract_scene(world: &World, aspect: f32) -> RenderScene {
    let camera = world
        .query_ref::<(&Camera, &GlobalTransform), ()>()
        .find(|(c, _)| !c.inactive)
        .map(|(c, g)| (*c, *g));
    let (view_projection, camera_position) = match camera {
        Some((cam, global)) => (cam.view_projection(&global, aspect), global.translation()),
        // No camera: look down −Z from slightly behind the origin so something is visible.
        None => {
            let cam = Camera::default();
            let at = GlobalTransform(kiln_math::Affine3A::from_translation(Vec3::new(
                0.0, 0.0, 5.0,
            )));
            (cam.view_projection(&at, aspect), at.translation())
        }
    };
    let light = world
        .query_ref::<(&DirectionalLight, &GlobalTransform), ()>()
        .next()
        .map(|(l, g)| DirectionalLightData {
            direction: g.forward(),
            color: l.color.map(|c| c * l.intensity),
        });
    let ambient = world
        .resource::<AmbientLight>()
        .copied()
        .unwrap_or_default();
    let clear = world.resource::<ClearColor>().copied().unwrap_or_default();
    let draws = world
        .query_ref::<(&MeshInstance, &GlobalTransform), ()>()
        .map(|(m, g)| DrawItem {
            mesh: m.mesh,
            material: m.material,
            transform: Mat4::from(g.0),
        })
        .collect();
    RenderScene {
        view_projection,
        camera_position,
        clear_color: clear.0,
        ambient: ambient.color.map(|c| c * ambient.intensity),
        light,
        draws,
    }
}

fn render_system(world: &mut World) {
    let size = world.resource::<WindowSize>().copied().unwrap_or_default();
    if !world.contains_resource::<Renderer>() {
        let Some(window) = world.resource::<PrimaryWindow>().cloned() else {
            return;
        };
        let settings = world
            .resource::<RendererSettings>()
            .cloned()
            .unwrap_or_default();
        match Renderer::new_windowed(&*window.window, size.physical.x, size.physical.y, &settings) {
            Ok(r) => {
                world.insert_resource(r);
            }
            Err(e) => {
                tracing::error!("failed to initialize the renderer: {e}");
                request_exit(world, AppExit::error());
                return;
            }
        }
    }
    if size.is_zero() {
        return;
    }
    let scene = extract_scene(world, size.aspect());
    let result = world.resource_scope(|world, renderer: &mut Renderer| {
        renderer.resize(size.physical.x, size.physical.y)?;
        let fallback;
        let assets = match world.resource::<AssetServer>() {
            Some(a) => a,
            None => {
                fallback = AssetServer::default();
                &fallback
            }
        };
        renderer.render(&scene, assets)
    });
    if let Some(Err(e)) = result {
        tracing::error!("rendering failed: {e}");
        request_exit(world, AppExit::error());
    }
}
