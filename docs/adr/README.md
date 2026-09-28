# Architecture decision records

Each ADR records one significant decision: its context, the choice, and the consequences.
ADRs are immutable once accepted; a change of direction gets a new ADR that supersedes the
old one.

| # | Title | Status |
|---|---|---|
| [0001](0001-rust-and-workspace.md) | Rust, cargo workspace, crate layering | Accepted |
| [0002](0002-vulkan-renderer.md) | Vulkan 1.3 renderer behind a thin RHI | Accepted |
| [0003](0003-coordinate-system.md) | Coordinate system, clip space and depth conventions | Accepted |
| [0004](0004-ecs-design.md) | Sparse-set ECS with exclusive-world systems | Accepted |
| [0005](0005-shader-pipeline.md) | WGSL shaders compiled to SPIR-V at build time with naga | Accepted |

To add one, copy an existing ADR, give it the next number, and set its status to *Proposed*.
