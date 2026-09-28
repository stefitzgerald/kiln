# Contributing to Kiln

Thanks for helping build Kiln! This guide covers setup, workflow and the quality bar every
change must meet.

## Setup

1. Install Rust 1.88+ and a Vulkan 1.3 driver. For validation layers, install the Vulkan
   SDK (Windows/macOS) or `vulkan-validationlayers` (Linux).
2. Run `cargo xtask doctor` and fix anything marked `[FAIL]`.
3. Run `cargo xtask ci` and `cargo xtask gpu-test`. Both should pass on a clean checkout.

## Workflow

- Branch from `main`; keep pull requests focused on one change.
- Discuss large or architectural changes in an issue first. Significant decisions get an
  [ADR](docs/adr/README.md).
- Every PR must pass CI: rustfmt, clippy with `-D warnings`, rustdoc with `-D warnings`,
  tests on Windows/Linux/macOS, GPU tests on lavapipe, the MSRV check and cargo-deny.

## Code standards

- **Tests come with features.** New behavior needs automated tests. If a case can only be
  verified by hand, add it to the [manual suites](docs/testing/manual/README.md) with
  clear steps and expected results. Give test cases IDs (`TC-AREA-NN`) and list them in
  the [test plan](docs/testing/M0-test-plan.md).
- **GPU tests** go in `tests/gpu.rs` of the relevant crate, marked
  `#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]`, and must assert
  zero validation errors and warnings.
- **Golden images**: after an intentional rendering change, run
  `cargo xtask bless-goldens`, **look at every changed PNG**, and explain the change in the
  PR.
- **No panics in library code for recoverable errors.** Return `Result` with a
  `thiserror` error type. `unwrap()` is linted in library code (tests are exempt).
- **`unsafe`** needs a `// SAFETY:` comment explaining why each invariant holds. Prefer
  safe abstractions; keep `unsafe` in the ECS internals and GPU backends.
- **Documentation**: every public item has rustdoc (`missing_docs` is enforced).
- Follow the conventions in [ADR 0003](docs/adr/0003-coordinate-system.md) (Y-up,
  right-handed, reverse-Z, linear color).

## Commit messages

Use the imperative mood ("Add shadow cascades"), a short summary line, and a body
explaining *why* when it is not obvious.

## License

By contributing, you agree that your contributions are dual-licensed under MIT OR
Apache-2.0, as described in the [README](README.md#license).
