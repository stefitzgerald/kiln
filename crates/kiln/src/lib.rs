//! **Kiln**: an open-source 3D game engine.
//!
//! This facade crate re-exports the engine crates and provides [`DefaultPlugins`], which set
//! up logging, a window, the scene systems and the renderer:
//!
//! ```no_run
//! use kiln::prelude::*;
//!
//! fn main() -> AppExit {
//!     App::new().add_plugin(DefaultPlugins::default()).run()
//! }
//! ```
//!
//! See `crates/kiln/examples/` for complete programs.

pub mod camera;
pub mod diagnostics;

pub use kiln_app as app;
pub use kiln_asset as asset;
pub use kiln_core as core;
pub use kiln_ecs as ecs;
pub use kiln_math as math;
pub use kiln_platform as platform;
pub use kiln_render as render;
pub use kiln_scene as scene;

use kiln_app::{App, Plugin};
use kiln_platform::{WindowPlugin, WindowSettings};
use kiln_render::{RenderPlugin, RendererSettings};
use kiln_scene::ScenePlugin;

/// Logging, window + input, scene systems and the renderer.
#[derive(Debug, Default)]
pub struct DefaultPlugins {
    /// Window configuration.
    pub window: WindowSettings,
    /// Renderer configuration.
    pub render: RendererSettings,
}

impl DefaultPlugins {
    /// Default plugins with a custom window title.
    pub fn titled(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            render: RendererSettings {
                app_name: title.clone(),
                ..Default::default()
            },
            window: WindowSettings {
                title,
                ..Default::default()
            },
        }
    }
}

impl Plugin for DefaultPlugins {
    fn build(&self, app: &mut App) {
        kiln_core::init_logging();
        tracing::info!(version = env!("CARGO_PKG_VERSION"), "Kiln starting");
        app.add_plugin(ScenePlugin)
            .add_plugin(WindowPlugin {
                settings: self.window.clone(),
            })
            .add_plugin(RenderPlugin {
                settings: self.render.clone(),
            });
        if let Some(smoke) = diagnostics::SmokeTestPlugin::from_env() {
            tracing::info!(frames = smoke.frames, "smoke test mode");
            app.add_plugin(smoke);
        }
    }
}

/// Everything needed for typical apps.
pub mod prelude {
    pub use crate::DefaultPlugins;
    pub use crate::camera::{FlyCamera, FlyCameraPlugin};
    pub use crate::diagnostics::{ExitOnEscPlugin, FpsTitlePlugin};
    pub use kiln_app::prelude::*;
    pub use kiln_asset::prelude::*;
    pub use kiln_core::{FixedClock, Time};
    pub use kiln_ecs::prelude::*;
    pub use kiln_math::{Aabb, Mat4, Quat, Vec2, Vec3, Vec4};
    pub use kiln_platform::prelude::*;
    pub use kiln_render::{PresentMode, RenderPlugin, Renderer, RendererSettings};
    pub use kiln_scene::prelude::*;
}

/// Compiles the code in the repository README as doctests so it cannot go stale.
#[doc = include_str!("../../../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;
