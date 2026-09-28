//! A free-flying editor-style camera controller.

use kiln_app::{App, Plugin, Stage};
use kiln_core::Time;
use kiln_ecs::{Component, World};
use kiln_math::{EulerRot, Quat, Vec3};
use kiln_platform::{ButtonInput, KeyCode, Mouse, MouseButton, PrimaryWindow};
use kiln_scene::Transform;

/// Hold the right mouse button to look around; WASD to move, Q/E (or Ctrl/Space) for down/up,
/// Shift to go faster, scroll wheel to change speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlyCamera {
    /// Movement speed in units per second.
    pub speed: f32,
    /// Radians per pixel of mouse motion.
    pub sensitivity: f32,
    /// Heading around +Y, radians.
    pub yaw: f32,
    /// Elevation, radians (clamped to ±89°).
    pub pitch: f32,
}

impl Component for FlyCamera {}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 3.0,
            sensitivity: 0.003,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

impl FlyCamera {
    /// Controller whose yaw/pitch match an existing rotation.
    pub fn from_rotation(rotation: Quat, speed: f32) -> Self {
        let (yaw, pitch, _) = rotation.to_euler(EulerRot::YXZ);
        Self {
            speed,
            yaw,
            pitch,
            ..Self::default()
        }
    }

    /// The controller's orientation.
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

/// Runs [`FlyCamera`] controllers in [`Stage::Update`].
#[derive(Debug, Default)]
pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_system(Stage::Update, fly_camera_system);
    }
}

const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

fn fly_camera_system(world: &mut World) {
    let dt = world.resource::<Time>().map_or(0.0, Time::delta_secs);
    let mouse = world.resource::<Mouse>().copied().unwrap_or_default();
    let (look, look_started, look_ended) =
        world
            .resource::<ButtonInput<MouseButton>>()
            .map_or((false, false, false), |b| {
                (
                    b.pressed(MouseButton::Right),
                    b.just_pressed(MouseButton::Right),
                    b.just_released(MouseButton::Right),
                )
            });
    let mut local = Vec3::ZERO;
    let mut fast = false;
    if let Some(keys) = world.resource::<ButtonInput<KeyCode>>() {
        let axis = |pos: &[KeyCode], neg: &[KeyCode]| {
            f32::from(u8::from(keys.any_pressed(pos.iter().copied())))
                - f32::from(u8::from(keys.any_pressed(neg.iter().copied())))
        };
        local.x = axis(&[KeyCode::KeyD], &[KeyCode::KeyA]);
        local.y = axis(
            &[KeyCode::KeyE, KeyCode::Space],
            &[KeyCode::KeyQ, KeyCode::ControlLeft],
        );
        local.z = axis(&[KeyCode::KeyS], &[KeyCode::KeyW]);
        fast = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    }

    if (look_started || look_ended)
        && let Some(w) = world.resource::<PrimaryWindow>()
    {
        w.window.set_cursor_visible(!look);
    }

    for (transform, cam) in world.query::<(&mut Transform, &mut FlyCamera)>() {
        if mouse.scroll != 0.0 {
            cam.speed = (cam.speed * 1.2f32.powf(mouse.scroll)).clamp(0.05, 1000.0);
        }
        if look {
            cam.yaw -= mouse.delta.x * cam.sensitivity;
            cam.pitch = (cam.pitch - mouse.delta.y * cam.sensitivity).clamp(-MAX_PITCH, MAX_PITCH);
        }
        transform.rotation = cam.rotation();
        if local != Vec3::ZERO {
            let speed = cam.speed * if fast { 4.0 } else { 1.0 };
            let step = transform.rotation * local.normalize() * speed * dt;
            transform.translation += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app() -> (App, kiln_ecs::Entity) {
        let mut app = App::new();
        app.add_plugin(FlyCameraPlugin)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Mouse>();
        let e = app.world.spawn((Transform::IDENTITY, FlyCamera::default()));
        (app, e)
    }

    #[test]
    fn w_moves_forward() {
        let (mut app, e) = app();
        app.world
            .resource_mut::<ButtonInput<KeyCode>>()
            .unwrap()
            .press(KeyCode::KeyW);
        app.update_with_delta(Duration::ZERO);
        app.update_with_delta(Duration::from_secs(1));
        let t = app.world.get::<Transform>(e).unwrap();
        assert!(
            t.translation.abs_diff_eq(Vec3::new(0.0, 0.0, -3.0), 1e-4),
            "{}",
            t.translation
        );
    }

    #[test]
    fn mouse_look_only_while_right_button_held() {
        let (mut app, e) = app();
        app.world.resource_mut::<Mouse>().unwrap().delta.x = 100.0;
        app.update_with_delta(Duration::from_millis(16));
        assert_eq!(app.world.get::<FlyCamera>(e).unwrap().yaw, 0.0);
        app.world
            .resource_mut::<ButtonInput<MouseButton>>()
            .unwrap()
            .press(MouseButton::Right);
        app.update_with_delta(Duration::from_millis(16));
        assert!(
            app.world.get::<FlyCamera>(e).unwrap().yaw < 0.0,
            "moving right turns right"
        );
        app.world.resource_mut::<Mouse>().unwrap().delta.y = -1e6;
        app.update_with_delta(Duration::from_millis(16));
        assert!(
            (app.world.get::<FlyCamera>(e).unwrap().pitch - MAX_PITCH).abs() < 1e-6,
            "pitch clamped"
        );
    }

    #[test]
    fn from_rotation_roundtrip() {
        let q = Quat::from_euler(EulerRot::YXZ, 0.7, -0.3, 0.0);
        let cam = FlyCamera::from_rotation(q, 1.0);
        assert!(cam.rotation().abs_diff_eq(q, 1e-5));
    }
}
