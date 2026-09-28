//! Application framework: [`App`], [`Plugin`]s, a staged schedule and the main loop.
//!
//! Each call to [`App::update`] runs one frame:
//!
//! ```text
//! Startup (first frame only)
//! First → PreUpdate → FixedUpdate × N → Update → PostUpdate → Render → Last
//! ```
//!
//! `FixedUpdate` runs zero or more times per frame, driven by the [`FixedTime`] resource
//! (60 Hz by default). Events registered with [`App::add_event`] are rotated after `Last`.

use std::any::type_name;
use std::collections::BTreeMap;
use std::num::NonZeroU8;
use std::time::{Duration, Instant};

use kiln_core::{FixedClock, Time};
use kiln_ecs::{Events, Resource, World};

/// A phase of the frame. Systems within a stage run in the order they were added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Once, before the first frame.
    Startup,
    /// Start of every frame.
    First,
    /// Input processing, event handling.
    PreUpdate,
    /// Deterministic simulation at a fixed rate; may run 0..N times per frame.
    FixedUpdate,
    /// Game logic.
    Update,
    /// Derived state, e.g. transform propagation.
    PostUpdate,
    /// Extraction and submission to the GPU.
    Render,
    /// End of frame.
    Last,
}

/// Why the app stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppExit {
    /// Normal shutdown (process exit code 0).
    #[default]
    Success,
    /// Failure with a non-zero process exit code.
    Error(NonZeroU8),
}

impl AppExit {
    /// Generic failure (exit code 1).
    pub const fn error() -> Self {
        AppExit::Error(NonZeroU8::MIN)
    }

    /// Process exit code.
    pub fn code(self) -> u8 {
        match self {
            AppExit::Success => 0,
            AppExit::Error(c) => c.get(),
        }
    }

    /// `true` for [`AppExit::Success`].
    pub fn is_success(self) -> bool {
        self == AppExit::Success
    }
}

impl std::process::Termination for AppExit {
    fn report(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self.code())
    }
}

/// Request that the app exits at the end of the current frame.
pub fn request_exit(world: &mut World, exit: AppExit) {
    world.init_resource::<Events<AppExit>>().send(exit);
}

/// Fixed-timestep state, available as a resource.
#[derive(Debug, Clone, Default)]
pub struct FixedTime {
    /// Accumulator deciding how many steps run each frame.
    pub clock: FixedClock,
    /// Timeline advanced by exactly one step per `FixedUpdate` run.
    pub time: Time,
}

impl FixedTime {
    /// Fixed state ticking at `hz`.
    pub fn from_hz(hz: u32) -> Self {
        Self {
            clock: FixedClock::from_hz(hz),
            time: Time::default(),
        }
    }

    /// Length of one fixed step.
    pub fn step(&self) -> Duration {
        self.clock.step()
    }
}

/// Error from [`App::try_add_plugin`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    /// The plugin was already added.
    #[error("plugin `{0}` was already added to this app")]
    DuplicatePlugin(String),
}

/// A reusable piece of app configuration: systems, resources, other plugins.
pub trait Plugin: Send + Sync + 'static {
    /// Configure the app.
    fn build(&self, app: &mut App);

    /// Unique name used to detect duplicates. Defaults to the type name.
    fn name(&self) -> &str {
        type_name::<Self>()
    }
}

/// A system: any `FnMut(&mut World)`.
type BoxedSystem = Box<dyn FnMut(&mut World) + Send>;

struct SystemEntry {
    name: String,
    run: BoxedSystem,
}

#[derive(Default)]
struct Schedule {
    stages: BTreeMap<Stage, Vec<SystemEntry>>,
}

impl Schedule {
    fn run(&mut self, stage: Stage, world: &mut World) {
        if let Some(systems) = self.stages.get_mut(&stage) {
            for system in systems {
                let _span = tracing::trace_span!("system", name = %system.name).entered();
                (system.run)(world);
            }
        }
    }
}

type Runner = Box<dyn FnOnce(App) -> AppExit>;

/// The application: a [`World`] plus the schedule that updates it.
pub struct App {
    /// All entities and resources.
    pub world: World,
    schedule: Schedule,
    plugins: Vec<String>,
    event_updaters: Vec<fn(&mut World)>,
    runner: Option<Runner>,
    started: bool,
    last_update: Option<Instant>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("world", &self.world)
            .field("plugins", &self.plugins)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// App with the core resources ([`Time`], [`FixedTime`], `Events<AppExit>`).
    pub fn new() -> Self {
        let mut app = Self {
            world: World::new(),
            schedule: Schedule::default(),
            plugins: Vec::new(),
            event_updaters: Vec::new(),
            runner: None,
            started: false,
            last_update: None,
        };
        app.init_resource::<Time>();
        app.init_resource::<FixedTime>();
        app.add_event::<AppExit>();
        app
    }

    /// Add a plugin.
    ///
    /// # Panics
    /// Panics if a plugin with the same [`Plugin::name`] was already added.
    pub fn add_plugin(&mut self, plugin: impl Plugin) -> &mut Self {
        if let Err(e) = self.try_add_plugin(plugin) {
            panic!("{e}");
        }
        self
    }

    /// Add a plugin, failing if it was already added.
    pub fn try_add_plugin(&mut self, plugin: impl Plugin) -> Result<&mut Self, AppError> {
        let name = plugin.name().to_owned();
        if self.plugins.contains(&name) {
            return Err(AppError::DuplicatePlugin(name));
        }
        tracing::debug!(plugin = %name, "adding plugin");
        self.plugins.push(name);
        plugin.build(self);
        Ok(self)
    }

    /// `true` if a plugin with this name has been added.
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p == name)
    }

    /// `true` if a plugin of type `P` (using the default name) has been added.
    pub fn has_plugin_type<P: Plugin>(&self) -> bool {
        self.has_plugin(type_name::<P>())
    }

    /// Names of added plugins, in build order.
    pub fn plugins(&self) -> &[String] {
        &self.plugins
    }

    /// Add a system to `stage`. Systems in a stage run in insertion order.
    pub fn add_system<F>(&mut self, stage: Stage, system: F) -> &mut Self
    where
        F: FnMut(&mut World) + Send + 'static,
    {
        let name = type_name::<F>().to_owned();
        self.schedule
            .stages
            .entry(stage)
            .or_default()
            .push(SystemEntry {
                name,
                run: Box::new(system),
            });
        self
    }

    /// Insert a resource.
    pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    /// Insert `R::default()` if absent.
    pub fn init_resource<R: Resource + Default>(&mut self) -> &mut Self {
        self.world.init_resource::<R>();
        self
    }

    /// Register an event type: inserts `Events<E>` and rotates it every frame.
    pub fn add_event<E: Send + Sync + 'static>(&mut self) -> &mut Self {
        if !self.world.contains_resource::<Events<E>>() {
            self.world.insert_resource(Events::<E>::default());
            self.event_updaters.push(|w| {
                if let Some(ev) = w.resource_mut::<Events<E>>() {
                    ev.update();
                }
            });
        }
        self
    }

    /// Replace the function that drives the main loop (e.g. a windowing event loop).
    pub fn set_runner(&mut self, runner: impl FnOnce(App) -> AppExit + 'static) -> &mut Self {
        self.runner = Some(Box::new(runner));
        self
    }

    /// Longest frame delta [`App::update`] reports. Longer stalls (debugger breaks, window
    /// drags, a minimized window) are clamped so animations and physics do not jump.
    pub const MAX_DELTA: Duration = Duration::from_millis(250);

    /// Run one frame, measuring real elapsed time since the previous frame
    /// (clamped to [`App::MAX_DELTA`]).
    pub fn update(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_update
            .map_or(Duration::ZERO, |t| (now - t).min(Self::MAX_DELTA));
        self.last_update = Some(now);
        self.update_with_delta(dt);
    }

    /// Run one frame as if `dt` elapsed. Deterministic; used by tests and replays.
    pub fn update_with_delta(&mut self, dt: Duration) {
        let _span = tracing::trace_span!("frame").entered();
        if !self.started {
            self.started = true;
            self.schedule.run(Stage::Startup, &mut self.world);
        }
        self.world.init_resource::<Time>().advance(dt);
        self.schedule.run(Stage::First, &mut self.world);
        self.schedule.run(Stage::PreUpdate, &mut self.world);

        let steps = self.world.init_resource::<FixedTime>().clock.accumulate(dt);
        for _ in 0..steps {
            let fixed = self.world.init_resource::<FixedTime>();
            let step = fixed.clock.step();
            fixed.time.advance(step);
            self.schedule.run(Stage::FixedUpdate, &mut self.world);
        }

        for stage in [Stage::Update, Stage::PostUpdate, Stage::Render, Stage::Last] {
            self.schedule.run(stage, &mut self.world);
        }
    }

    /// Rotate event buffers. Called by runners after [`App::update`] and after reading
    /// [`App::exit_requested`], so exit requests are never missed.
    pub fn end_frame(&mut self) {
        for update in &self.event_updaters {
            update(&mut self.world);
        }
    }

    /// The first pending exit request, if any.
    pub fn exit_requested(&self) -> Option<AppExit> {
        self.world
            .resource::<Events<AppExit>>()?
            .iter()
            .next()
            .copied()
    }

    /// Run the app until it exits, using the configured runner (or a headless loop).
    pub fn run(&mut self) -> AppExit {
        let mut app = std::mem::take(self);
        let runner = app.runner.take().unwrap_or_else(|| Box::new(run_headless));
        runner(app)
    }
}

/// Default runner: update as fast as possible until an [`AppExit`] is requested.
pub fn run_headless(mut app: App) -> AppExit {
    loop {
        app.update();
        if let Some(exit) = app.exit_requested() {
            return exit;
        }
        app.end_frame();
    }
}

/// Commonly used items.
pub mod prelude {
    pub use crate::{App, AppExit, FixedTime, Plugin, Stage, request_exit};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Counter(u32);

    const FRAME: Duration = Duration::from_millis(16);

    /// TC-APP-01: plugins build in order; duplicates are rejected.
    #[test]
    fn tc_app_01_plugin_order_and_duplicates() {
        let log = Arc::new(Mutex::new(Vec::new()));
        struct Named(&'static str, Arc<Mutex<Vec<&'static str>>>);
        impl Plugin for Named {
            fn build(&self, app: &mut App) {
                self.1.lock().unwrap().push(self.0);
                let (name, log) = (self.0, self.1.clone());
                app.add_system(Stage::Update, move |_| log.lock().unwrap().push(name));
            }
            fn name(&self) -> &str {
                self.0
            }
        }
        let mut app = App::new();
        app.add_plugin(Named("a", log.clone()))
            .add_plugin(Named("b", log.clone()));
        let err = app.try_add_plugin(Named("a", log.clone())).unwrap_err();
        assert_eq!(err, AppError::DuplicatePlugin("a".into()));
        assert!(err.to_string().contains("already added"));
        app.update_with_delta(FRAME);
        assert_eq!(*log.lock().unwrap(), ["a", "b", "a", "b"]);
        assert_eq!(app.plugins(), ["a", "b"]);
        assert!(app.has_plugin("b"));
    }

    #[test]
    #[should_panic(expected = "already added")]
    fn tc_app_01_duplicate_plugin_panics() {
        struct P;
        impl Plugin for P {
            fn build(&self, _: &mut App) {}
        }
        App::new().add_plugin(P).add_plugin(P);
    }

    /// TC-APP-02: startup systems run exactly once.
    #[test]
    fn tc_app_02_startup_runs_once() {
        let mut app = App::new();
        app.init_resource::<Counter>();
        app.add_system(Stage::Startup, |w| {
            w.resource_mut::<Counter>().unwrap().0 += 1
        });
        for _ in 0..10 {
            app.update_with_delta(FRAME);
            app.end_frame();
        }
        assert_eq!(app.world.resource::<Counter>().unwrap().0, 1);
        assert_eq!(app.world.resource::<Time>().unwrap().frame(), 10);
    }

    /// TC-APP-03: 100 ms at 60 Hz runs FixedUpdate exactly 6 times.
    #[test]
    fn tc_app_03_fixed_update_count() {
        let mut app = App::new();
        app.init_resource::<Counter>();
        app.add_system(Stage::FixedUpdate, |w| {
            w.resource_mut::<Counter>().unwrap().0 += 1
        });
        app.update_with_delta(Duration::from_millis(100));
        assert_eq!(app.world.resource::<Counter>().unwrap().0, 6);
        let fixed = app.world.resource::<FixedTime>().unwrap();
        assert_eq!(fixed.time.frame(), 6);
    }

    /// Stage order is deterministic.
    #[test]
    fn stages_run_in_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut app = App::new();
        for stage in [
            Stage::Last,
            Stage::Render,
            Stage::PostUpdate,
            Stage::Update,
            Stage::FixedUpdate,
            Stage::PreUpdate,
            Stage::First,
            Stage::Startup,
        ] {
            let order = order.clone();
            app.add_system(stage, move |_| order.lock().unwrap().push(stage));
        }
        app.update_with_delta(Duration::from_millis(17));
        let got = order.lock().unwrap().clone();
        assert_eq!(
            got,
            [
                Stage::Startup,
                Stage::First,
                Stage::PreUpdate,
                Stage::FixedUpdate,
                Stage::Update,
                Stage::PostUpdate,
                Stage::Render,
                Stage::Last
            ]
        );
    }

    /// TC-APP-04: an exit request stops the headless runner with its exit code.
    #[test]
    fn tc_app_04_exit_request_stops_runner() {
        let mut app = App::new();
        app.init_resource::<Counter>();
        app.add_system(Stage::Update, |w| {
            let c = &mut w.resource_mut::<Counter>().unwrap().0;
            *c += 1;
            if *c == 5 {
                request_exit(w, AppExit::Success);
            }
        });
        assert_eq!(app.run(), AppExit::Success);

        let mut failing = App::new();
        failing.add_system(Stage::Update, |w| request_exit(w, AppExit::error()));
        let exit = failing.run();
        assert_eq!(exit.code(), 1);
        assert!(!exit.is_success());
    }

    #[test]
    fn custom_runner_is_used() {
        let mut app = App::new();
        app.set_runner(|mut app| {
            app.update_with_delta(FRAME);
            AppExit::error()
        });
        assert_eq!(app.run(), AppExit::error());
    }
}
