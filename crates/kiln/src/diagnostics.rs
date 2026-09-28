//! Small quality-of-life plugins for examples and tools.

use std::time::Duration;

use kiln_app::{App, AppExit, Plugin, Stage, request_exit};
use kiln_core::Time;
use kiln_ecs::World;
use kiln_platform::{ButtonInput, KeyCode, PrimaryWindow, WindowSettings};

/// Exit the app when Escape is pressed.
#[derive(Debug, Default)]
pub struct ExitOnEscPlugin;

impl Plugin for ExitOnEscPlugin {
    fn build(&self, app: &mut App) {
        app.add_system(Stage::PreUpdate, |world: &mut World| {
            if world
                .resource::<ButtonInput<KeyCode>>()
                .is_some_and(|k| k.just_pressed(KeyCode::Escape))
            {
                request_exit(world, AppExit::Success);
            }
        });
    }
}

/// Show frames per second and frame time in the window title, updated twice a second.
#[derive(Debug, Default)]
pub struct FpsTitlePlugin;

#[derive(Debug, Default)]
struct FpsCounter {
    frames: u32,
    elapsed: Duration,
}

const FPS_INTERVAL: Duration = Duration::from_millis(500);

impl Plugin for FpsTitlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FpsCounter>()
            .add_system(Stage::Last, |world: &mut World| {
                let dt = world.resource::<Time>().map_or(Duration::ZERO, Time::delta);
                let Some(counter) = world.resource_mut::<FpsCounter>() else {
                    return;
                };
                counter.frames += 1;
                counter.elapsed += dt;
                if counter.elapsed < FPS_INTERVAL {
                    return;
                }
                let secs = counter.elapsed.as_secs_f64();
                let fps = f64::from(counter.frames) / secs;
                let ms = secs * 1000.0 / f64::from(counter.frames);
                *counter = FpsCounter::default();
                let base = world
                    .resource::<WindowSettings>()
                    .map_or("Kiln".into(), |s| s.title.clone());
                if let Some(w) = world.resource::<PrimaryWindow>() {
                    w.set_title(&format!("{base} | {fps:.0} FPS ({ms:.2} ms)"));
                }
            });
    }
}

/// Environment variable: when set to `N`, the app exits after rendering `N` frames, with a
/// failure code if the renderer reported any validation errors. Used for smoke tests.
pub const SMOKE_FRAMES_ENV: &str = "KILN_SMOKE_FRAMES";

/// Exits after a fixed number of rendered frames (see [`SMOKE_FRAMES_ENV`]).
#[derive(Debug)]
pub struct SmokeTestPlugin {
    /// Frames to render before exiting.
    pub frames: u32,
}

impl SmokeTestPlugin {
    /// Plugin configured from [`SMOKE_FRAMES_ENV`], if set.
    pub fn from_env() -> Option<Self> {
        std::env::var(SMOKE_FRAMES_ENV)
            .ok()?
            .trim()
            .parse()
            .ok()
            .map(|frames| Self { frames })
    }
}

impl Plugin for SmokeTestPlugin {
    fn build(&self, app: &mut App) {
        let target = self.frames;
        let mut rendered = 0u32;
        app.add_system(Stage::Last, move |world: &mut World| {
            let Some(renderer) = world.resource::<kiln_render::Renderer>() else {
                return;
            };
            rendered += 1;
            if rendered < target {
                return;
            }
            let stats = renderer.validation_stats();
            let render = renderer.stats();
            tracing::info!(frames = rendered, ?stats, ?render, "smoke test finished");
            let exit = if stats.errors == 0 {
                AppExit::Success
            } else {
                AppExit::error()
            };
            request_exit(world, exit);
        });
    }
}
