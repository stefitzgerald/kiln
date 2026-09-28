//! TC-MAN-04: the window is a solid cornflower blue.
//!
//! `cargo run --example clear_color`

use kiln::prelude::*;

/// Cornflower blue (sRGB 100, 149, 237) in linear space.
const CORNFLOWER: [f32; 4] = [0.127, 0.301, 0.847, 1.0];

fn main() -> AppExit {
    App::new()
        .add_plugin(DefaultPlugins::titled("Kiln: clear color"))
        .add_plugin(ExitOnEscPlugin)
        .insert_resource(ClearColor(CORNFLOWER))
        .run()
}
