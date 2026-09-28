//! Platform layer: a window, OS events and input, driven by `winit`.
//!
//! Add [`WindowPlugin`] to an [`App`] to open a window and run the app from the OS event
//! loop. Resources provided:
//!
//! * [`PrimaryWindow`]: the window handle (inserted once the window exists).
//! * [`WindowSize`]: current size and DPI scale.
//! * [`ButtonInput<KeyCode>`], [`ButtonInput<MouseButton>`] and [`Mouse`].
//!
//! Closing the window requests [`AppExit::Success`].

pub mod input;

use std::sync::Arc;
use std::time::{Duration, Instant};

use kiln_app::{App, AppExit, Plugin, request_exit};
use kiln_ecs::World;
use kiln_math::{UVec2, Vec2};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

pub use input::{ButtonInput, KeyCode, Mouse, MouseButton};
pub use winit;

/// Window configuration, read when the window is created.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSettings {
    /// Title bar text.
    pub title: String,
    /// Initial client width in logical pixels.
    pub width: u32,
    /// Initial client height in logical pixels.
    pub height: u32,
    /// Allow the user to resize.
    pub resizable: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { title: "Kiln".into(), width: 1280, height: 720, resizable: true }
    }
}

/// The application window (resource).
#[derive(Debug, Clone)]
pub struct PrimaryWindow {
    /// Shared window handle; implements the `raw-window-handle` traits.
    pub window: Arc<Window>,
}

impl PrimaryWindow {
    /// Set the title bar text.
    pub fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }
}

/// Current window size (resource).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowSize {
    /// Client area in physical pixels. Zero while minimized.
    pub physical: UVec2,
    /// DPI scale factor (physical / logical).
    pub scale_factor: f64,
}

impl WindowSize {
    /// `true` when there is nothing to render into (e.g. minimized).
    pub fn is_zero(&self) -> bool {
        self.physical.x == 0 || self.physical.y == 0
    }

    /// Width / height, or 1.0 when minimized.
    pub fn aspect(&self) -> f32 {
        if self.is_zero() { 1.0 } else { self.physical.x as f32 / self.physical.y as f32 }
    }
}

/// Opens a window and drives the app from the OS event loop.
#[derive(Debug, Default)]
pub struct WindowPlugin {
    /// Window configuration.
    pub settings: WindowSettings,
}

impl Plugin for WindowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .init_resource::<WindowSize>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Mouse>()
            .set_runner(run_winit);
    }
}

/// How often the loop wakes while minimized, so the app stays responsive at near-zero CPU.
const MINIMIZED_POLL: Duration = Duration::from_millis(100);

struct Runner {
    app: App,
    window: Option<Arc<Window>>,
    exit: Option<AppExit>,
}

impl Runner {
    fn world(&mut self) -> &mut World {
        &mut self.app.world
    }

    fn minimized(&self) -> bool {
        self.app.world.resource::<WindowSize>().is_none_or(WindowSize::is_zero)
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        self.app.update();
        if let Some(exit) = self.app.exit_requested() {
            self.exit = Some(exit);
            event_loop.exit();
        }
        self.app.end_frame();
        let w = self.world();
        if let Some(k) = w.resource_mut::<ButtonInput<KeyCode>>() {
            k.clear_frame();
        }
        if let Some(m) = w.resource_mut::<ButtonInput<MouseButton>>() {
            m.clear_frame();
        }
        if let Some(m) = w.resource_mut::<Mouse>() {
            m.clear_frame();
        }
    }

    fn set_size(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.world().insert_resource(WindowSize {
            physical: UVec2::new(size.width, size.height),
            scale_factor,
        });
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let settings = self.app.world.resource::<WindowSettings>().cloned().unwrap_or_default();
        let attrs = Window::default_attributes()
            .with_title(settings.title)
            .with_inner_size(LogicalSize::new(settings.width, settings.height))
            .with_resizable(settings.resizable);
        match event_loop.create_window(attrs) {
            Ok(window) => {
                let window = Arc::new(window);
                tracing::info!(
                    size = ?window.inner_size(),
                    scale = window.scale_factor(),
                    "window created"
                );
                self.set_size(window.inner_size(), window.scale_factor());
                self.world().insert_resource(PrimaryWindow { window: window.clone() });
                window.request_redraw();
                self.window = Some(window);
            }
            Err(e) => {
                tracing::error!("failed to create window: {e}");
                self.exit = Some(AppExit::error());
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => request_exit(self.world(), AppExit::Success),
            WindowEvent::Resized(size) => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                self.set_size(size, scale);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(size) = self.window.as_ref().map(|w| w.inner_size()) {
                    self.set_size(size, scale_factor);
                }
            }
            WindowEvent::Focused(false) => {
                let w = self.world();
                if let Some(k) = w.resource_mut::<ButtonInput<KeyCode>>() {
                    k.release_all();
                }
                if let Some(m) = w.resource_mut::<ButtonInput<MouseButton>>() {
                    m.release_all();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(keys) = self.world().resource_mut::<ButtonInput<KeyCode>>() {
                        match event.state {
                            ElementState::Pressed => keys.press(code),
                            ElementState::Released => keys.release(code),
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(buttons) = self.world().resource_mut::<ButtonInput<MouseButton>>() {
                    match state {
                        ElementState::Pressed => buttons.press(button),
                        ElementState::Released => buttons.release(button),
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(m) = self.world().resource_mut::<Mouse>() {
                    m.position = Some(Vec2::new(position.x as f32, position.y as f32));
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(m) = self.world().resource_mut::<Mouse>() {
                    m.position = None;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let Some(m) = self.world().resource_mut::<Mouse>() {
                    m.scroll += lines;
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.minimized() {
                    self.frame(event_loop);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if let Some(m) = self.world().resource_mut::<Mouse>() {
                m.delta += Vec2::new(delta.0 as f32, delta.1 as f32);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.minimized() {
            // Nothing to draw; sleep until an event or the next poll so the app can still
            // react (e.g. to an exit request) without burning a CPU core.
            if let Some(exit) = self.app.exit_requested() {
                self.exit = Some(exit);
                event_loop.exit();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + MINIMIZED_POLL));
        } else {
            event_loop.set_control_flow(ControlFlow::Poll);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

/// Runner installed by [`WindowPlugin`].
pub fn run_winit(app: App) -> AppExit {
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            tracing::error!("failed to create the OS event loop: {e}");
            return AppExit::error();
        }
    };
    let mut runner = Runner { app, window: None, exit: None };
    if let Err(e) = event_loop.run_app(&mut runner) {
        tracing::error!("event loop error: {e}");
        return AppExit::error();
    }
    // Drop the app (and with it GPU resources) before the window.
    let exit = runner.exit.unwrap_or_default();
    drop(runner.app);
    exit
}

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        ButtonInput, KeyCode, Mouse, MouseButton, PrimaryWindow, WindowPlugin, WindowSettings,
        WindowSize,
    };
}
