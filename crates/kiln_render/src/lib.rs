//! Kiln's renderer: a Vulkan 1.3 forward renderer using dynamic rendering.
//!
//! M0 features: depth-tested meshes, base color factor/texture/vertex color, Lambert
//! lighting from one directional light plus ambient, unlit and double-sided materials,
//! frustum culling, window presentation and headless offscreen rendering with readback.
//!
//! Use [`RenderPlugin`] with the ECS, or drive a [`Renderer`] directly with a
//! [`RenderScene`] (see the GPU tests for headless examples).

mod gpu_types;
mod pipeline;
mod plugin;
mod renderer;

pub use kiln_rhi::{AdapterInfo, MemoryReport, PresentMode, Validation, ValidationStats};
pub use plugin::{RenderPlugin, extract_scene};
pub use renderer::{
    DirectionalLightData, DrawItem, FRAMES_IN_FLIGHT, FrameStatus, RenderScene, RenderStats,
    Renderer, RendererSettings,
};

/// Rendering errors.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// A Vulkan backend error.
    #[error(transparent)]
    Vulkan(#[from] kiln_rhi_vulkan::VkError),
    /// A built-in shader failed to load.
    #[error("shader error: {0}")]
    Shader(String),
    /// The window handle could not be obtained.
    #[error("window handle unavailable: {0}")]
    Window(String),
    /// Readback was requested from a windowed renderer.
    #[error("operation requires a headless renderer")]
    NotHeadless,
    /// An internal invariant was violated.
    #[error("internal renderer error: {0}")]
    Internal(String),
}
