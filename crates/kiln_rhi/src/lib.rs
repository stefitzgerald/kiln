//! Backend-agnostic rendering types shared by Kiln's GPU backends and the renderer.
//!
//! M0 ships a single backend (Vulkan). This crate holds the vocabulary that does not depend
//! on it, so a second backend (e.g. WebGPU) can be added without changing the renderer's
//! public API. The full device trait abstraction is deliberately deferred until a second
//! backend exists (see `docs/adr/0002-vulkan-renderer.md`).

/// Size of a 2D render target, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent2d {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Extent2d {
    /// New extent.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// `true` if either side is zero (e.g. a minimized window).
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Width / height (1.0 when empty).
    pub fn aspect(&self) -> f32 {
        if self.is_empty() {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }
}

/// Swapchain presentation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PresentMode {
    /// V-sync, never tears. Always supported.
    #[default]
    Fifo,
    /// V-sync with the latest frame replacing queued ones (low latency, no tearing).
    /// Falls back to `Fifo` if unsupported.
    Mailbox,
    /// No v-sync; may tear. Falls back to `Fifo` if unsupported.
    Immediate,
}

/// Kind of GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    /// Dedicated GPU.
    Discrete,
    /// GPU integrated with the CPU.
    Integrated,
    /// Software rasterizer (e.g. lavapipe, SwiftShader).
    Cpu,
    /// Virtualized GPU.
    Virtual,
    /// Anything else.
    Other,
}

/// Information about the selected GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    /// Device name reported by the driver.
    pub name: String,
    /// Kind of device.
    pub device_type: DeviceType,
    /// PCI vendor id.
    pub vendor_id: u32,
    /// Supported API version as `(major, minor, patch)`.
    pub api_version: (u32, u32, u32),
    /// Driver name and version string, when available.
    pub driver: String,
}

/// How the GPU backend should use validation (debug) layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Validation {
    /// Enabled in debug builds when the layer is installed; overridable with the
    /// `KILN_VALIDATION` environment variable (`0`, `1` or `required`).
    #[default]
    Auto,
    /// Enable if installed.
    Enabled,
    /// Fail device creation if the layer is not installed. Used by CI.
    Required,
    /// Never enable.
    Disabled,
}

impl Validation {
    /// Resolve [`Validation::Auto`] using the build profile and `KILN_VALIDATION`.
    pub fn resolve(self) -> Validation {
        if self != Validation::Auto {
            return self;
        }
        match std::env::var("KILN_VALIDATION")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("0" | "off" | "false") => Validation::Disabled,
            Some("1" | "on" | "true") => Validation::Enabled,
            Some("required") => Validation::Required,
            _ if cfg!(debug_assertions) => Validation::Enabled,
            _ => Validation::Disabled,
        }
    }
}

/// Counts of validation messages seen since the device was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ValidationStats {
    /// `true` if a validation layer is active.
    pub enabled: bool,
    /// Validation errors reported.
    pub errors: u32,
    /// Validation warnings reported.
    pub warnings: u32,
}

/// GPU memory usage summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryReport {
    /// Bytes in live allocations.
    pub allocated_bytes: u64,
    /// Bytes reserved from the driver (including free space in blocks).
    pub reserved_bytes: u64,
    /// Number of live allocations.
    pub allocation_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent() {
        assert!(Extent2d::new(0, 10).is_empty());
        assert_eq!(Extent2d::new(0, 10).aspect(), 1.0);
        assert_eq!(Extent2d::new(200, 100).aspect(), 2.0);
    }

    #[test]
    fn explicit_validation_is_not_overridden() {
        assert_eq!(Validation::Required.resolve(), Validation::Required);
        assert_eq!(Validation::Disabled.resolve(), Validation::Disabled);
    }
}
