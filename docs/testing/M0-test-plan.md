# Milestone 0 test plan

M0 delivers the engine foundation and a basic 3D scene: core types, math, ECS, app loop,
windowing and input, glTF import, scene hierarchy and a Vulkan 1.3 forward renderer.

Every test case has an ID. Automated cases name the test function that implements them
(search the repo for the ID, e.g. `tc_ecs_03`). Manual cases are in
[`manual/`](manual/README.md).

## How to run

| What | Command | Needs |
|---|---|---|
| Environment check | `cargo xtask doctor` | — |
| All CPU tests | `cargo test --workspace` | — |
| Full CI locally | `cargo xtask ci` | cargo-deny (optional) |
| GPU tests | `cargo xtask gpu-test` | Vulkan 1.3 GPU or lavapipe |
| Re-render goldens | `cargo xtask bless-goldens` | as above; **inspect the PNGs before committing** |
| Example smoke test | `KILN_SMOKE_FRAMES=120 cargo run --example spinning_cube` | GPU + display |

GPU tests are marked `#[ignore]` so `cargo test` works on machines without a GPU;
`cargo xtask gpu-test` runs them with `--ignored`. Every GPU test fails if the Vulkan
validation layer reports **any** error or warning (TC-GPU-08). A dedicated test
(`validation_counter_detects_errors`) proves the counter really catches errors.

Environment variables: `KILN_LOG` (log filter, e.g. `debug`), `KILN_VALIDATION`
(`0`/`1`/`required`), `KILN_GPU` (prefer a GPU by name substring), `KILN_BLESS=1`
(write goldens), `KILN_SMOKE_FRAMES=N` (exit after N frames, non-zero on validation errors).

## Pass criteria for M0

1. Every automated case below passes locally and in CI (Windows, Linux, macOS; GPU suite
   on Linux lavapipe).
2. Every manual case passes on at least one Windows machine with a discrete GPU, recorded
   in [`manual/results-M0.md`](manual/results-M0.md).
3. `cargo xtask ci` is clean: rustfmt, clippy `-D warnings`, rustdoc `-D warnings`,
   cargo-deny.

---

## Build and tooling (manual)

| ID | Steps | Expected |
|---|---|---|
| TC-BLD-01 | `cargo xtask doctor` | Every line `[ ok ]` or `[warn]`; any `[FAIL]` names the fix |
| TC-BLD-02 | Fresh clone → `cargo build --workspace` | Succeeds, zero warnings |
| TC-BLD-03 | `cargo xtask ci` | Ends with `All CI checks passed.` |
| TC-BLD-04 | Push a branch / open a PR | All CI jobs green: lint, test ×3 OS, GPU (lavapipe), MSRV, cargo-deny |

## kiln_core

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-CORE-01 | `handle::tc_core_01_*` | Insert, remove, insert | Slot reused with generation +1; stale handle resolves to `None` |
| TC-CORE-02 | `handle::tc_core_02_*` | 64 × ≤10k random insert/remove ops vs `HashMap` model | Always identical |
| TC-CORE-03 | `time::tc_core_03_*` | 60 Hz clock, feed 50 ms | Exactly 3 steps, remainder < 1 µs |
| TC-CORE-04 | `time::tc_core_04_*` | Feed a 5 s hitch | Clamped to 8 steps; backlog discarded |
| TC-CORE-05 | `log::tc_core_05_*` | `init_logging()` from 9 threads | No panic; consistent result |

## kiln_math

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-MATH-01 | `transform::tc_math_01_*` | Property test: `T * T⁻¹` | Identity within 1e-4 |
| TC-MATH-02 | `projection::tc_math_02_*` | Reverse-Z (finite, infinite, ortho) | Near → 1, far → 0 |
| TC-MATH-03 | `projection::tc_math_03_*` | `look_at` from (0,0,5) | Forward −Z, up +Y |
| TC-MATH-04 | `bounds::tc_math_04_*` | AABB rotated 90°/45° about Y | Correct enclosing box |
| TC-MATH-05 | `bounds::tc_math_05_*` | Frustum vs boxes inside/outside/straddling | Inside / Outside / Intersect |

## kiln_ecs

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-ECS-01 | `tc_ecs_01_*` | Spawn, despawn, spawn | Same index, new generation; stale id sees nothing |
| TC-ECS-02 | `tc_ecs_02_*` | `(Entity, &A, &mut B)` over mixed entities | Only matches visited; writes persist |
| TC-ECS-03 | `tc_ecs_03_*` | `With` / `Without` / `Has` | Correct subsets |
| TC-ECS-04 | `tc_ecs_04_*` | Add/remove components at runtime | Query membership follows |
| TC-ECS-05 | `tc_ecs_05_*` | Despawn/insert via `Commands` while iterating | Applied only on `apply`; no panic |
| TC-ECS-06 | `tc_ecs_06_*` | Resources incl. `resource_scope` | Insert/get/mutate/remove work |
| TC-ECS-07 | `tc_ecs_07_*` | `(&mut A, &A)`, `(&mut A, &mut A)` | Rejected **when the query is created** with `ConflictingAccess` naming the component |
| TC-ECS-08 | `cargo bench -p kiln_ecs` | 100k-entity queries, spawn/despawn | Baseline recorded; compare across changes |
| — | `world_matches_model` | 64 × 500 random ops vs model | Always identical |

## kiln_scene

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-SCN-01 | `tc_scn_01_*` | Parent (1,0,0), child (0,1,0) | Child global (1,1,0) |
| TC-SCN-02 | `tc_scn_02_*` | Parent rotated 90° Y, scaled 2 | Child global (0,0,−5), matches matrix product |
| TC-SCN-03 | `tc_scn_03_*` | Reparent, then un-parent | Both sides of the hierarchy updated |
| TC-SCN-04 | `tc_scn_04_*` | Parent A under its descendant, or itself | `Err(Cycle)`; hierarchy unchanged |
| TC-SCN-05 | `tc_scn_05_*` | Recursive despawn of a 7-entity subtree | All gone; surviving parent cleaned up |
| TC-SCN-06 | `tc_scn_06_*` | 4-level chain; move root | Every level correct |

## kiln_app / kiln_platform

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-APP-01 | `tc_app_01_*` | Two plugins; duplicate plugin | Build order kept; duplicate → error / panic with clear message |
| TC-APP-02 | `tc_app_02_*` | Startup system over 10 frames | Runs once |
| TC-APP-03 | `tc_app_03_*` | 100 ms frame at 60 Hz | FixedUpdate runs exactly 6× |
| TC-APP-04 | `tc_app_04_*` | Exit request (success / error) | Runner returns it; exit code 0 / 1 |
| TC-INP-01 | `tc_inp_01_*` | Press, next frame, OS key-repeat | just_pressed on frame 1 only |
| TC-INP-02 | `tc_inp_02_*` | Release | just_released for exactly one frame |
| TC-INP-03 | `tc_inp_03_*` | Focus lost with buttons held | Everything released |

## kiln_asset

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-AST-01 | `tc_ast_01_*` | Load `Box.glb` | 1 mesh, 24 vertices, 36 indices, AABB ±0.5 |
| TC-AST-02 | `tc_ast_02_*` | Same file via two path spellings | Same handle; parsed once |
| TC-AST-03 | `tc_ast_03_*` | Missing file | `NotFound` naming the path |
| TC-AST-04 | `tc_ast_04_*` | Every truncation + 300 random corruptions | `Parse` errors only; never panics |
| TC-AST-05 | `tc_ast_05_*` | Primitive without normals | Flat unit normals generated; default material |
| TC-AST-06 | `tc_ast_06_*` | `CheckerCube.glb` | 64×64 sRGB base color texture, UVs |
| TC-AST-07 | `tc_ast_07_*` (asset + scene) | Node hierarchy | Mirrored in the ECS; globals propagated |
| TC-AST-08 | `kiln_render` GPU test | Render `CheckerCube.glb` | Both checker colors visible; matches golden |

## kiln_rhi_vulkan / kiln_render (GPU)

| ID | Test | Case | Expected |
|---|---|---|---|
| TC-GPU-01 | `tc_gpu_01_*` | Headless context | Device created and named; validation active in CI |
| TC-GPU-02 | `tc_gpu_02_*` | Clear (0.2, 0.4, 0.6) and read back | Every pixel = sRGB-encoded value ±1 |
| TC-GPU-03 | `tc_gpu_03_*` | RGB triangle | Red vertex at **top** (Y-up), green bottom-left, blue bottom-right; golden |
| TC-GPU-04 | `tc_gpu_04_*` | Lit cube from (2,2,2) | Front faces lit (proves winding/culling); golden |
| TC-GPU-05 | `tc_gpu_05_*` | Overlapping quads, both draw orders | Identical images; near quad wins (reverse-Z) |
| TC-GPU-06 | `tc_gpu_06_*` | Sphere lit from +X | Lit side ≫ shadow side; golden |
| TC-GPU-07 | `tc_gpu_07_*` | 300 frames × 50 draws, 2 frames in flight | GPU memory identical at frame 10 and 300 |
| TC-GPU-08 | all GPU tests | — | 0 validation errors and warnings |
| — | `frustum_culling_and_missing_assets` | Off-screen and missing assets | Culled count correct; missing assets skipped or defaulted |
| — | `headless_resize` | Resize offscreen target | New size; zero size ignored |

Golden images live in `tests/assets/goldens/`. A test fails if more than 0.5 % of pixels
differ by more than 2 in any channel; diffs are written to `target/golden-diff/`.
Goldens were blessed on an AMD RX 9070 XT. If lavapipe in CI differs beyond tolerance,
download the `golden-diff` artifact, confirm the difference is rasterization noise, and
widen the tolerance or bless per-platform. Do not bless blindly.

## Manual suites

See [`manual/README.md`](manual/README.md): TC-MAN-01 … TC-MAN-11 (windowing, resizing,
minimize, examples, fly camera, large models, DPI changes, error handling).
