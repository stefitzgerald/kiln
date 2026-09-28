//! Keyboard and mouse input state.

use std::collections::HashSet;
use std::hash::Hash;

use kiln_math::Vec2;

pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Pressed / just-pressed / just-released state for a set of buttons.
///
/// The platform runner calls [`ButtonInput::press`] and [`ButtonInput::release`] as OS
/// events arrive and [`ButtonInput::clear_frame`] after every frame, so `just_*` queries
/// are true for exactly one frame.
#[derive(Debug, Clone)]
pub struct ButtonInput<T: Copy + Eq + Hash> {
    pressed: HashSet<T>,
    just_pressed: HashSet<T>,
    just_released: HashSet<T>,
}

impl<T: Copy + Eq + Hash> Default for ButtonInput<T> {
    fn default() -> Self {
        Self { pressed: HashSet::new(), just_pressed: HashSet::new(), just_released: HashSet::new() }
    }
}

impl<T: Copy + Eq + Hash> ButtonInput<T> {
    /// Register a press. OS key-repeat presses of an already held button are ignored.
    pub fn press(&mut self, button: T) {
        if self.pressed.insert(button) {
            self.just_pressed.insert(button);
        }
    }

    /// Register a release.
    pub fn release(&mut self, button: T) {
        if self.pressed.remove(&button) {
            self.just_released.insert(button);
        }
    }

    /// Release every held button (e.g. when the window loses focus).
    pub fn release_all(&mut self) {
        self.just_released.extend(self.pressed.drain());
    }

    /// Held down right now.
    pub fn pressed(&self, button: T) -> bool {
        self.pressed.contains(&button)
    }

    /// Went down this frame.
    pub fn just_pressed(&self, button: T) -> bool {
        self.just_pressed.contains(&button)
    }

    /// Went up this frame.
    pub fn just_released(&self, button: T) -> bool {
        self.just_released.contains(&button)
    }

    /// Any of `buttons` held.
    pub fn any_pressed(&self, buttons: impl IntoIterator<Item = T>) -> bool {
        buttons.into_iter().any(|b| self.pressed(b))
    }

    /// Iterate held buttons.
    pub fn get_pressed(&self) -> impl Iterator<Item = &T> {
        self.pressed.iter()
    }

    /// Forget per-frame transitions. Called at the end of each frame.
    pub fn clear_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }
}

/// Mouse state for the current frame (resource).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mouse {
    /// Cursor position in physical pixels from the window's top-left, if inside the window.
    pub position: Option<Vec2>,
    /// Raw motion this frame (device units, unaffected by cursor clamping).
    pub delta: Vec2,
    /// Scroll this frame, in lines (positive = away from the user).
    pub scroll: f32,
}

impl Mouse {
    /// Reset per-frame accumulators.
    pub fn clear_frame(&mut self) {
        self.delta = Vec2::ZERO;
        self.scroll = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-INP-01
    #[test]
    fn tc_inp_01_press_then_hold() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(keys.just_pressed(KeyCode::KeyW));
        keys.clear_frame();
        // OS key repeat while held must not re-trigger just_pressed.
        keys.press(KeyCode::KeyW);
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(!keys.just_pressed(KeyCode::KeyW));
    }

    /// TC-INP-02
    #[test]
    fn tc_inp_02_release_is_one_frame() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Space);
        keys.clear_frame();
        keys.release(KeyCode::Space);
        assert!(!keys.pressed(KeyCode::Space));
        assert!(keys.just_released(KeyCode::Space));
        keys.clear_frame();
        assert!(!keys.just_released(KeyCode::Space));
        // Releasing something never pressed is ignored.
        keys.release(KeyCode::KeyQ);
        assert!(!keys.just_released(KeyCode::KeyQ));
    }

    /// TC-INP-03
    #[test]
    fn tc_inp_03_focus_loss_releases_everything() {
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        buttons.press(MouseButton::Left);
        buttons.clear_frame();
        buttons.release_all();
        assert_eq!(buttons.get_pressed().count(), 0);
        assert!(buttons.just_released(MouseButton::Right));
        assert!(buttons.just_released(MouseButton::Left));
    }

    #[test]
    fn press_and_release_same_frame() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyE);
        keys.release(KeyCode::KeyE);
        assert!(keys.just_pressed(KeyCode::KeyE) && keys.just_released(KeyCode::KeyE));
        assert!(!keys.pressed(KeyCode::KeyE));
    }
}
