# Kiln

An open-source 3D game engine written in Rust, with a Vulkan 1.3 renderer.

> **Status: Milestone 0 (foundation).** Kiln can open a window, load glTF 2.0 models into
> an ECS scene graph, and render them with depth, textures and directional lighting. It is
> not yet ready for making games. See the [roadmap](#roadmap).

## Features (M0)

- **ECS**: generational entities, sparse-set storage, typed queries with borrow-conflict
  detection, resources, events, deferred commands
- **App framework**: plugins, staged schedule, deterministic fixed timestep
- **Scene**: transform hierarchy with cycle-safe reparenting, cameras (perspective and
  orthographic, reverse-Z), directional and ambient light
- **Assets**: glTF 2.0 (`.gltf` and `.glb`) meshes, materials, textures and node
  hierarchy; robust against malformed files
- **Renderer**: Vulkan 1.3 with dynamic rendering, mipmapped sRGB textures, frustum
  culling, headless mode for automated image tests, validation layers in debug builds
- **Platform**: Windows, Linux and macOS windowing and input via winit
- **Tooling**: `cargo xtask doctor | ci | gpu-test | bless-goldens`, golden-image GPU tests,
  and CI on three operating systems plus a software-GPU job

## Quick start

Prerequisites:

- Rust 1.88 or newer ([rustup](https://rustup.rs)).
  - On Windows, use either the MSVC toolchain (with Visual Studio Build Tools and the C++
    workload) or the GNU toolchain (`rustup default stable-x86_64-pc-windows-gnu` with
    MinGW-w64 gcc on PATH).
- A GPU and driver with **Vulkan 1.3**.
- Optional: the [Vulkan SDK](https://vulkan.lunarg.com), for validation layers during
  development.

```bash
cargo xtask doctor
```

```bash
cargo run --example spinning_cube
```

```bash
cargo run --example gltf_viewer -- tests/assets/CheckerCube.glb
```

Other examples: `hello_window`, `clear_color`, `triangle`. In the viewer, hold the right
mouse button to look, move with WASD/Q/E, hold Shift to go faster, and scroll to change
speed.

## A minimal app

```rust,no_run
use kiln::prelude::*;

fn main() -> AppExit {
    let mut app = App::new();
    app.add_plugin(DefaultPlugins::titled("My game"));

    let assets = app.world.init_resource::<AssetServer>();
    let mesh = assets.meshes.add(Mesh::cube(1.0));
    let material = assets.materials.add(Material::color([0.9, 0.4, 0.1, 1.0]));

    app.world.spawn((Transform::IDENTITY, MeshInstance { mesh, material }));
    app.world.spawn((
        Transform::from_xyz(2.0, 2.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        Camera::default(),
    ));
    app.world.spawn((
        Transform::IDENTITY.looking_at(Vec3::new(-1.0, -2.0, -1.0), Vec3::Y),
        DirectionalLight::default(),
    ));
    app.run()
}
```

## Repository layout

| Path | Contents |
|---|---|
| `crates/kiln` | Facade crate: re-exports, `DefaultPlugins`, fly camera, examples |
| `crates/kiln_core` | Handles, time, logging |
| `crates/kiln_math` | Transforms, bounds, frusta, projections (on `glam`) |
| `crates/kiln_ecs` | Entity-component-system |
| `crates/kiln_app` | App, plugins, schedule |
| `crates/kiln_asset` | Meshes, materials, images, glTF import |
| `crates/kiln_scene` | Hierarchy, transforms, cameras, lights |
| `crates/kiln_platform` | Window and input (winit) |
| `crates/kiln_rhi`, `kiln_rhi_vulkan` | GPU abstraction types and the Vulkan backend |
| `crates/kiln_render` | Renderer and WGSL shaders |
| `xtask` | Developer commands |
| `docs/adr` | Architecture decision records |
| `docs/testing` | Test plan and manual test suites |

## Testing

```bash
cargo test --workspace
```

```bash
cargo xtask gpu-test
```

```bash
cargo xtask ci
```

The [M0 test plan](docs/testing/M0-test-plan.md) lists every test case by ID, and the
[manual suites](docs/testing/manual/README.md) cover interactive checks.

## Roadmap

- **M0: Foundation** (current): everything above.
- **M1: Rendering**: metallic-roughness PBR, normal maps, shadows, HDR and tone mapping,
  MSAA, alpha blending and masking, KHR_texture_transform.
- **M2: Engine core**: parallel system scheduler, change detection, component derive
  macro, asynchronous asset loading with hot reload, scene serialization.
- **M3: Gameplay**: physics integration, audio, input actions, animation (skinning,
  morph targets).
- **M4: Editor**: scene editor, inspector, asset browser.
- Later: additional backends (WebGPU, Metal/D3D12 as needed), mobile, scripting.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Participation is governed by the
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Kiln is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in Kiln by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.

Third-party test assets keep their own licenses; see
[tests/assets/README.md](tests/assets/README.md).
