use ash::vk;
use kiln_rhi::PresentMode;

use crate::{GpuContext, ResultExt, VkError, VkResult};

/// A window surface. Destroyed on drop (before the context's instance).
pub struct Surface {
    ctx: GpuContext,
    raw: vk::SurfaceKHR,
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surface").field("raw", &self.raw).finish()
    }
}

impl Surface {
    pub(crate) fn from_raw(ctx: GpuContext, raw: vk::SurfaceKHR) -> Self {
        Self { ctx, raw }
    }

    /// Vulkan handle.
    pub fn raw(&self) -> vk::SurfaceKHR {
        self.raw
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the swapchain that used this surface is destroyed first (it owns us).
        unsafe { self.ctx.surface_fn().destroy_surface(self.raw, None) };
    }
}

/// An image acquired from the swapchain.
#[derive(Debug, Clone, Copy)]
pub struct AcquiredImage {
    /// Index into [`Swapchain::images`].
    pub index: u32,
    /// The image.
    pub image: vk::Image,
    /// Its view.
    pub view: vk::ImageView,
    /// Signal this after rendering; present waits on it.
    pub render_finished: vk::Semaphore,
    /// The swapchain still works but should be recreated.
    pub suboptimal: bool,
}

/// Presentation swapchain for a [`Surface`].
pub struct Swapchain {
    ctx: GpuContext,
    raw: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    /// One per image: the image's acquire→present hand-off cannot reuse a semaphore that a
    /// pending present may still wait on.
    render_finished: Vec<vk::Semaphore>,
    format: vk::SurfaceFormatKHR,
    extent: vk::Extent2D,
    present_mode: vk::PresentModeKHR,
    requested_mode: PresentMode,
    // Declared last: destroyed after the swapchain.
    surface: Surface,
}

impl std::fmt::Debug for Swapchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Swapchain")
            .field("extent", &self.extent)
            .field("format", &self.format.format)
            .field("present_mode", &self.present_mode)
            .field("images", &self.images.len())
            .finish()
    }
}

impl Swapchain {
    /// Create a swapchain of roughly `extent` (clamped to what the surface allows).
    pub fn new(ctx: &GpuContext, surface: Surface, extent: vk::Extent2D, mode: PresentMode) -> VkResult<Self> {
        let mut sc = Self {
            ctx: ctx.clone(),
            raw: vk::SwapchainKHR::null(),
            images: Vec::new(),
            views: Vec::new(),
            render_finished: Vec::new(),
            format: vk::SurfaceFormatKHR::default(),
            extent,
            present_mode: vk::PresentModeKHR::FIFO,
            requested_mode: mode,
            surface,
        };
        sc.recreate(extent)?;
        Ok(sc)
    }

    /// Rebuild for a new size. Waits for the GPU to go idle first. A zero `extent` is
    /// rejected; skip rendering while minimized instead.
    pub fn recreate(&mut self, extent: vk::Extent2D) -> VkResult<()> {
        if extent.width == 0 || extent.height == 0 {
            return Err(VkError::InvalidArgument("swapchain extent is zero".into()));
        }
        self.ctx.wait_idle()?;
        let surface_fn = self.ctx.surface_fn();
        let pd = self.ctx.physical_device();
        let surface = self.surface.raw;
        // SAFETY: plain queries on live objects.
        let (caps, formats, modes) = unsafe {
            (
                surface_fn.get_physical_device_surface_capabilities(pd, surface).ctx("surface capabilities")?,
                surface_fn.get_physical_device_surface_formats(pd, surface).ctx("surface formats")?,
                surface_fn.get_physical_device_surface_present_modes(pd, surface).ctx("present modes")?,
            )
        };
        let format = pick_format(&formats)
            .ok_or_else(|| VkError::InvalidArgument("surface reports no formats".into()))?;
        let present_mode = pick_present_mode(self.requested_mode, &modes);
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: extent.width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: extent.height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        if extent.width == 0 || extent.height == 0 {
            return Err(VkError::InvalidArgument("surface extent is zero".into()));
        }
        let mut image_count = caps.min_image_count + 1;
        if caps.max_image_count > 0 {
            image_count = image_count.min(caps.max_image_count);
        }
        let composite = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::INHERIT,
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        ]
        .into_iter()
        .find(|c| caps.supported_composite_alpha.contains(*c))
        .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);

        // TRANSFER_SRC enables screenshots, but only request it where supported.
        let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT
            | (caps.supported_usage_flags & vk::ImageUsageFlags::TRANSFER_SRC);
        let old = self.raw;
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(usage)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(composite)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(old);
        let swapchain_fn = self.swapchain_fn()?.clone();
        // SAFETY: valid create info; `old` is retired by this call.
        let raw = unsafe { swapchain_fn.create_swapchain(&info, None) }.ctx("create swapchain")?;
        self.destroy_views();
        if old != vk::SwapchainKHR::null() {
            // SAFETY: the device is idle, so the old swapchain's images are unused.
            unsafe { swapchain_fn.destroy_swapchain(old, None) };
        }
        self.raw = raw;
        // SAFETY: fresh swapchain.
        self.images = unsafe { swapchain_fn.get_swapchain_images(raw) }.ctx("get swapchain images")?;
        let device = self.ctx.device();
        for &image in &self.images {
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format.format)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1),
                );
            // SAFETY: valid image.
            self.views.push(unsafe { device.create_image_view(&view_info, None) }.ctx("swapchain view")?);
            // SAFETY: valid device.
            self.render_finished.push(
                unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                    .ctx("swapchain semaphore")?,
            );
        }
        self.format = format;
        self.extent = extent;
        self.present_mode = present_mode;
        tracing::info!(
            width = extent.width,
            height = extent.height,
            format = ?format.format,
            present_mode = ?present_mode,
            images = self.images.len(),
            "swapchain created"
        );
        Ok(())
    }

    fn swapchain_fn(&self) -> VkResult<&ash::khr::swapchain::Device> {
        self.ctx
            .0
            .swapchain_fn
            .as_ref()
            .ok_or_else(|| VkError::InvalidArgument("context was created without a surface".into()))
    }

    fn destroy_views(&mut self) {
        let device = self.ctx.device();
        // SAFETY: callers ensure the GPU is idle.
        unsafe {
            for v in self.views.drain(..) {
                device.destroy_image_view(v, None);
            }
            for s in self.render_finished.drain(..) {
                device.destroy_semaphore(s, None);
            }
        }
    }

    /// Acquire the next image, signalling `image_available` when it is ready.
    /// Returns `Ok(None)` if the swapchain is out of date and must be recreated.
    pub fn acquire(&self, image_available: vk::Semaphore) -> VkResult<Option<AcquiredImage>> {
        let swapchain_fn = self.swapchain_fn()?;
        // SAFETY: valid swapchain and unsignaled semaphore.
        match unsafe { swapchain_fn.acquire_next_image(self.raw, u64::MAX, image_available, vk::Fence::null()) } {
            Ok((index, suboptimal)) => {
                let i = index as usize;
                Ok(Some(AcquiredImage {
                    index,
                    image: self.images[i],
                    view: self.views[i],
                    render_finished: self.render_finished[i],
                    suboptimal,
                }))
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(None),
            Err(e) => Err(VkError::Api { context: "acquire swapchain image", result: e }),
        }
    }

    /// Present `image`. Returns `true` if the swapchain should be recreated.
    pub fn present(&self, image: &AcquiredImage) -> VkResult<bool> {
        let wait = [image.render_finished];
        let swapchains = [self.raw];
        let indices = [image.index];
        let info = vk::PresentInfoKHR::default()
            .wait_semaphores(&wait)
            .swapchains(&swapchains)
            .image_indices(&indices);
        match self.ctx.present(&info) {
            Ok(suboptimal) => Ok(suboptimal || image.suboptimal),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(true),
            Err(e) => Err(VkError::Api { context: "present", result: e }),
        }
    }

    /// Current size.
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    /// Image format.
    pub fn format(&self) -> vk::Format {
        self.format.format
    }

    /// Active present mode.
    pub fn present_mode(&self) -> vk::PresentModeKHR {
        self.present_mode
    }

    /// Swapchain images.
    pub fn images(&self) -> &[vk::Image] {
        &self.images
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        let _ = self.ctx.wait_idle();
        self.destroy_views();
        if let Ok(f) = self.swapchain_fn() {
            // SAFETY: device idle; swapchain no longer used.
            unsafe { f.destroy_swapchain(self.raw, None) };
        }
    }
}

fn pick_format(formats: &[vk::SurfaceFormatKHR]) -> Option<vk::SurfaceFormatKHR> {
    let preferred = [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB];
    preferred
        .iter()
        .find_map(|p| {
            formats
                .iter()
                .find(|f| f.format == *p && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
        })
        .or_else(|| {
            let first = formats.first();
            if let Some(f) = first {
                tracing::warn!(format = ?f.format, "no sRGB swapchain format; colors may look washed out");
            }
            first
        })
        .copied()
}

fn pick_present_mode(requested: PresentMode, available: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    let want = match requested {
        PresentMode::Fifo => vk::PresentModeKHR::FIFO,
        PresentMode::Mailbox => vk::PresentModeKHR::MAILBOX,
        PresentMode::Immediate => vk::PresentModeKHR::IMMEDIATE,
    };
    if available.contains(&want) { want } else { vk::PresentModeKHR::FIFO }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_preference() {
        let f = |format| vk::SurfaceFormatKHR { format, color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR };
        assert_eq!(pick_format(&[f(vk::Format::B8G8R8A8_UNORM), f(vk::Format::B8G8R8A8_SRGB)]).unwrap().format, vk::Format::B8G8R8A8_SRGB);
        assert_eq!(pick_format(&[f(vk::Format::A2B10G10R10_UNORM_PACK32)]).unwrap().format, vk::Format::A2B10G10R10_UNORM_PACK32);
        assert!(pick_format(&[]).is_none());
    }

    #[test]
    fn present_mode_fallback() {
        let avail = [vk::PresentModeKHR::FIFO, vk::PresentModeKHR::IMMEDIATE];
        assert_eq!(pick_present_mode(PresentMode::Mailbox, &avail), vk::PresentModeKHR::FIFO);
        assert_eq!(pick_present_mode(PresentMode::Immediate, &avail), vk::PresentModeKHR::IMMEDIATE);
    }
}
