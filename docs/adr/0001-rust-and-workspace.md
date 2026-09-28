# ADR 0001: Rust, cargo workspace, crate layering

**Status:** Accepted (M0)

## Context
Kiln aims to be a production-grade, open-source 3D engine comparable to Godot. The core
language choice drives safety, tooling, contributor pool and platform reach.

## Decision
- Implement the engine in **Rust** (edition 2024, MSRV 1.88, verified in CI).
- One **cargo workspace**; each subsystem is its own crate with a one-way dependency order:

  ```
  kiln_core ─┬─ kiln_math ─────────────┐
             ├─ kiln_ecs ── kiln_app ──┼─ kiln_scene ─┬─ kiln_render ── kiln (facade)
             │              kiln_asset ┘              │
             │              kiln_platform ────────────┤
             └─ kiln_rhi ── kiln_rhi_vulkan ──────────┘
  ```
- `kiln` is the user-facing facade (re-exports, `DefaultPlugins`, prelude).
- Dual license **MIT OR Apache-2.0**. Dependencies are limited to permissive licenses and
  checked by `cargo-deny`.
- Developer workflows live in `cargo xtask` (doctor, ci, gpu-test, bless-goldens,
  gen/fetch-assets) so they work the same on every OS without shell scripts.
- Lints are workspace-wide: `missing_docs`, `unsafe_op_in_unsafe_fn`,
  `clippy::undocumented_unsafe_blocks`, and `clippy::unwrap_used` in library code.
  CI runs with `-D warnings`.

## Consequences
- Memory safety by default. `unsafe` is concentrated in the ECS query internals and the
  Vulkan backend, and every block carries a `SAFETY:` justification.
- Crate boundaries keep compile times down and let the scene and ECS be tested without a
  GPU.
- No `rust-toolchain.toml` is committed. Contributors on Windows may use either the MSVC
  or the GNU host toolchain, and a pinned channel would force one of them. `cargo xtask
  doctor` detects a missing or shadowed linker.
