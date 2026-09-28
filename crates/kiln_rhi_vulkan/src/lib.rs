//! Vulkan 1.3 backend for Kiln.
//!
//! * [`GpuContext`]: instance, validation messenger, device, queue and memory allocator.
//! * [`Buffer`] / [`Texture`]: RAII GPU resources that free themselves on drop.
//! * [`Swapchain`]: presentation to a window surface, with recreation on resize.
//!
//! Requirements: Vulkan 1.3 with `dynamicRendering` and `synchronization2`. On macOS the
//! loader's portability enumeration is enabled so MoltenVK-class drivers are found.
//!
//! Environment variables:
//! * `KILN_VALIDATION` = `0` | `1` | `required`: override validation layer usage.
//! * `KILN_GPU` = substring: prefer the GPU whose name contains it (e.g. `llvmpipe`).

mod context;
mod debug;
mod resources;
mod swapchain;
pub mod util;

pub use ash;
pub use ash::vk;
pub use context::{ContextDesc, GpuContext};
pub use gpu_allocator::MemoryLocation;
pub use resources::{Buffer, BufferDesc, Texture, TextureDesc};
pub use swapchain::{AcquiredImage, Surface, Swapchain};

/// Errors from the Vulkan backend.
#[derive(Debug, thiserror::Error)]
pub enum VkError {
    /// The Vulkan loader library could not be loaded.
    #[error("failed to load the Vulkan library (is a Vulkan driver installed?): {0}")]
    Loading(String),
    /// A Vulkan call returned an error.
    #[error("{context}: {result}")]
    Api {
        /// What was being attempted.
        context: &'static str,
        /// Result code.
        result: vk::Result,
    },
    /// The loader or every device is older than Vulkan 1.3.
    #[error("Vulkan 1.3 is required but the loader only supports {0}.{1}")]
    UnsupportedVersion(u32, u32),
    /// No device met the requirements.
    #[error("no suitable GPU found: {0}")]
    NoSuitableDevice(String),
    /// Validation was required but the layer is missing.
    #[error(
        "validation was required but VK_LAYER_KHRONOS_validation is not installed \
         (install the Vulkan SDK or your distribution's vulkan-validationlayers package)"
    )]
    ValidationUnavailable,
    /// GPU memory allocation failed.
    #[error("GPU memory allocation failed: {0}")]
    Allocation(String),
    /// Invalid argument passed to the backend.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

/// Result alias for this crate.
pub type VkResult<T> = Result<T, VkError>;

pub(crate) trait ResultExt<T> {
    fn ctx(self, context: &'static str) -> VkResult<T>;
}

impl<T> ResultExt<T> for Result<T, vk::Result> {
    fn ctx(self, context: &'static str) -> VkResult<T> {
        self.map_err(|result| VkError::Api { context, result })
    }
}
