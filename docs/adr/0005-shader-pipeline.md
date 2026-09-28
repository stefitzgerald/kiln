# ADR 0005: WGSL shaders compiled to SPIR-V at build time with naga

**Status:** Accepted (M0)

## Context
Vulkan consumes SPIR-V. Options: GLSL or HLSL through glslc/dxc (needs the Vulkan SDK on
every build machine), Slang, or WGSL through naga (pure Rust).

## Decision
- Shaders are written in **WGSL** (`crates/kiln_render/shaders/*.wgsl`).
- `build.rs` parses and **validates** them with **naga** and writes one SPIR-V module per
  entry point to `OUT_DIR`. The renderer embeds them with `include_bytes!`. A shader
  error fails the build with a source-annotated message.
- naga's coordinate-space flip is disabled because the backend flips Y with the viewport
  (ADR 0003).

## Consequences
- Building needs no Vulkan SDK or external tools, so CI and contributors need no setup.
- The same WGSL sources can feed a future WebGPU backend.
- WGSL lacks some features found in GLSL/HLSL/Slang (e.g. certain extensions). If a
  feature requires it, add a second compiler path behind the same `OUT_DIR` convention.
- Runtime shader hot-reload is not in M0. It is planned for the editor milestone.
