//! TC-MAN-01..03: opens a 1280×720 window titled "Kiln". Close it with the X button or Esc.
//!
//! `cargo run --example hello_window`

use kiln::prelude::*;

fn main() -> AppExit {
    App::new()
        .add_plugin(DefaultPlugins::titled("Kiln"))
        .add_plugin(ExitOnEscPlugin)
        .run()
}
