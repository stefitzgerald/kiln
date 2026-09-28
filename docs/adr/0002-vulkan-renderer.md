# ADR 0002: Vulkan 1.3 renderer behind a thin RHI

**Status:** Accepted (M0)

## Context
The renderer needs low-level control (memory, synchronization, command recording) and
must run on Windows, Linux and macOS. Options considered: wgpu (portable, higher level),
a custom RHI with several backends, or Vulkan only.

## Decision
- Ship a single **Vulkan 1.3** backend (`kiln_rhi_vulkan`, via `ash`) using **dynamic
  rendering** and **synchronization2**. There are no render pass or framebuffer objects.
- macOS runs through a Vulkan-on-Metal driver. The loader's portability enumeration and
  `VK_KHR_portability_subset` are enabled automatically.
- Memory is managed by `gpu-allocator`. `Buffer` and `Texture` are RAII types that hold a
  reference-counted `GpuContext`, so the device always outlives its resources.
- `kiln_rhi` holds the backend-agnostic vocabulary (extents, present modes, adapter
  info, validation and memory stats). A full device-trait abstraction is **deferred until
  a second backend exists**, so it can be shaped by two real implementations instead of
  guesses.
- Validation layers are on by default in debug builds (`KILN_VALIDATION` overrides). A
  debug messenger routes messages to `tracing` and **counts** errors and warnings; every
  GPU test asserts both are zero.
- Headless offscreen rendering with CPU readback is a first-class target, used by the
  golden-image tests.
- Frames in flight: 2. Per-image present semaphores avoid reusing a semaphore that is
  still in use by the presentation engine.

## Consequences
- No browser (WebGPU) target in M0. The WGSL shader choice (ADR 0005) keeps that path
  open.
- CI runs GPU tests on Mesa lavapipe (a software Vulkan driver) with validation layers
  required.
- The backend owns queue submission locking. The renderer is single-threaded in M0.
