use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};

use crate::util::image_barrier;
use crate::{GpuContext, ResultExt, VkError, VkResult};

/// Parameters for [`Buffer::new`].
#[derive(Debug, Clone)]
pub struct BufferDesc<'a> {
    /// Debug name.
    pub name: &'a str,
    /// Size in bytes (> 0).
    pub size: u64,
    /// Vulkan usage flags.
    pub usage: vk::BufferUsageFlags,
    /// Where the memory lives. `CpuToGpu` and `GpuToCpu` buffers are persistently mapped.
    pub location: MemoryLocation,
}

/// A GPU buffer and its memory. Freed on drop.
pub struct Buffer {
    ctx: GpuContext,
    raw: vk::Buffer,
    allocation: Option<Allocation>,
    size: u64,
}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer").field("raw", &self.raw).field("size", &self.size).finish()
    }
}

impl Buffer {
    /// Create a buffer.
    pub fn new(ctx: &GpuContext, desc: &BufferDesc<'_>) -> VkResult<Self> {
        if desc.size == 0 {
            return Err(VkError::InvalidArgument(format!("buffer `{}` has size 0", desc.name)));
        }
        let device = ctx.device();
        let info = vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(desc.usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: valid device and create info.
        let raw = unsafe { device.create_buffer(&info, None) }.ctx("create buffer")?;
        // SAFETY: `raw` was just created.
        let requirements = unsafe { device.get_buffer_memory_requirements(raw) };
        let allocation = ctx.allocator().allocate(&AllocationCreateDesc {
            name: desc.name,
            requirements,
            location: desc.location,
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        });
        let allocation = match allocation {
            Ok(a) => a,
            Err(e) => {
                // SAFETY: unused buffer.
                unsafe { device.destroy_buffer(raw, None) };
                return Err(VkError::Allocation(format!("{}: {e}", desc.name)));
            }
        };
        // SAFETY: memory and offset come from a live allocation sized for this buffer.
        if let Err(e) = unsafe { device.bind_buffer_memory(raw, allocation.memory(), allocation.offset()) } {
            // SAFETY: unused buffer.
            unsafe { device.destroy_buffer(raw, None) };
            let _ = ctx.allocator().free(allocation);
            return Err(VkError::Api { context: "bind buffer memory", result: e });
        }
        Ok(Self { ctx: ctx.clone(), raw, allocation: Some(allocation), size: desc.size })
    }

    /// Create a device-local buffer initialized with `data` via a staging copy.
    pub fn with_data(
        ctx: &GpuContext,
        name: &str,
        usage: vk::BufferUsageFlags,
        data: &[u8],
    ) -> VkResult<Self> {
        let size = data.len() as u64;
        let mut staging = Buffer::new(ctx, &BufferDesc {
            name: "staging",
            size,
            usage: vk::BufferUsageFlags::TRANSFER_SRC,
            location: MemoryLocation::CpuToGpu,
        })?;
        staging.write(0, data)?;
        let buffer = Buffer::new(ctx, &BufferDesc {
            name,
            size,
            usage: usage | vk::BufferUsageFlags::TRANSFER_DST,
            location: MemoryLocation::GpuOnly,
        })?;
        ctx.immediate_submit(|cmd| {
            let region = [vk::BufferCopy::default().size(size)];
            // SAFETY: both buffers are live and large enough.
            unsafe { ctx.device().cmd_copy_buffer(cmd, staging.raw, buffer.raw, &region) };
        })?;
        Ok(buffer)
    }

    /// Vulkan handle.
    pub fn raw(&self) -> vk::Buffer {
        self.raw
    }

    /// Size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Mapped memory, for host-visible buffers.
    pub fn mapped(&self) -> Option<&[u8]> {
        self.allocation.as_ref()?.mapped_slice()
    }

    /// Mapped memory, for host-visible buffers.
    pub fn mapped_mut(&mut self) -> Option<&mut [u8]> {
        self.allocation.as_mut()?.mapped_slice_mut()
    }

    /// Copy `data` into a host-visible buffer at `offset`.
    pub fn write(&mut self, offset: u64, data: &[u8]) -> VkResult<()> {
        let size = self.size;
        let mapped = self
            .mapped_mut()
            .ok_or_else(|| VkError::InvalidArgument("buffer is not host-visible".into()))?;
        let start = offset as usize;
        let end = start
            .checked_add(data.len())
            .filter(|&e| e as u64 <= size)
            .ok_or_else(|| VkError::InvalidArgument(format!("write of {} bytes at {offset} exceeds {size}", data.len())))?;
        mapped[start..end].copy_from_slice(data);
        Ok(())
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: owners guarantee the GPU no longer uses the buffer (they wait on the frame
        // fence or device idle before dropping).
        unsafe { self.ctx.device().destroy_buffer(self.raw, None) };
        if let Some(a) = self.allocation.take() {
            if let Err(e) = self.ctx.allocator().free(a) {
                tracing::error!("failed to free buffer memory: {e}");
            }
        }
    }
}

/// Parameters for [`Texture::new`].
#[derive(Debug, Clone)]
pub struct TextureDesc<'a> {
    /// Debug name.
    pub name: &'a str,
    /// Size in pixels.
    pub extent: vk::Extent2D,
    /// Pixel format.
    pub format: vk::Format,
    /// Usage flags.
    pub usage: vk::ImageUsageFlags,
    /// Mip levels (≥ 1).
    pub mip_levels: u32,
    /// COLOR or DEPTH.
    pub aspect: vk::ImageAspectFlags,
}

/// A 2D GPU image with a view covering all mips. Freed on drop.
pub struct Texture {
    ctx: GpuContext,
    raw: vk::Image,
    view: vk::ImageView,
    allocation: Option<Allocation>,
    extent: vk::Extent2D,
    format: vk::Format,
    mip_levels: u32,
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Texture")
            .field("extent", &self.extent)
            .field("format", &self.format)
            .field("mip_levels", &self.mip_levels)
            .finish()
    }
}

impl Texture {
    /// Create an uninitialized image.
    pub fn new(ctx: &GpuContext, desc: &TextureDesc<'_>) -> VkResult<Self> {
        if desc.extent.width == 0 || desc.extent.height == 0 {
            return Err(VkError::InvalidArgument(format!("texture `{}` has zero extent", desc.name)));
        }
        let device = ctx.device();
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(desc.format)
            .extent(vk::Extent3D { width: desc.extent.width, height: desc.extent.height, depth: 1 })
            .mip_levels(desc.mip_levels.max(1))
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(desc.usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: valid device and create info.
        let raw = unsafe { device.create_image(&info, None) }.ctx("create image")?;
        // SAFETY: `raw` was just created.
        let requirements = unsafe { device.get_image_memory_requirements(raw) };
        let allocation = match ctx.allocator().allocate(&AllocationCreateDesc {
            name: desc.name,
            requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        }) {
            Ok(a) => a,
            Err(e) => {
                // SAFETY: unused image.
                unsafe { device.destroy_image(raw, None) };
                return Err(VkError::Allocation(format!("{}: {e}", desc.name)));
            }
        };
        let cleanup = |allocation: Allocation| {
            // SAFETY: unused image.
            unsafe { device.destroy_image(raw, None) };
            let _ = ctx.allocator().free(allocation);
        };
        // SAFETY: memory from a live allocation sized for this image.
        if let Err(e) = unsafe { device.bind_image_memory(raw, allocation.memory(), allocation.offset()) } {
            cleanup(allocation);
            return Err(VkError::Api { context: "bind image memory", result: e });
        }
        let view_info = vk::ImageViewCreateInfo::default()
            .image(raw)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(desc.format)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(desc.aspect)
                    .level_count(desc.mip_levels.max(1))
                    .layer_count(1),
            );
        // SAFETY: valid image.
        let view = match unsafe { device.create_image_view(&view_info, None) } {
            Ok(v) => v,
            Err(e) => {
                cleanup(allocation);
                return Err(VkError::Api { context: "create image view", result: e });
            }
        };
        Ok(Self {
            ctx: ctx.clone(),
            raw,
            view,
            allocation: Some(allocation),
            extent: desc.extent,
            format: desc.format,
            mip_levels: desc.mip_levels.max(1),
        })
    }

    /// Upload tightly packed RGBA8 pixels into a new sampled texture, generating a full mip
    /// chain when the format supports linear blits. Final layout: `SHADER_READ_ONLY_OPTIMAL`.
    pub fn from_rgba8(
        ctx: &GpuContext,
        name: &str,
        width: u32,
        height: u32,
        pixels: &[u8],
        srgb: bool,
    ) -> VkResult<Self> {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(VkError::InvalidArgument(format!(
                "texture `{name}`: expected {expected} bytes, got {}",
                pixels.len()
            )));
        }
        let format = if srgb { vk::Format::R8G8B8A8_SRGB } else { vk::Format::R8G8B8A8_UNORM };
        // SAFETY: plain query.
        let props = unsafe {
            ctx.instance().get_physical_device_format_properties(ctx.physical_device(), format)
        };
        let can_blit = props.optimal_tiling_features.contains(
            vk::FormatFeatureFlags::BLIT_SRC
                | vk::FormatFeatureFlags::BLIT_DST
                | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR,
        );
        let mip_levels = if can_blit { 32 - width.max(height).max(1).leading_zeros() } else { 1 };

        let mut staging = Buffer::new(ctx, &BufferDesc {
            name: "texture staging",
            size: pixels.len() as u64,
            usage: vk::BufferUsageFlags::TRANSFER_SRC,
            location: MemoryLocation::CpuToGpu,
        })?;
        staging.write(0, pixels)?;
        let texture = Texture::new(ctx, &TextureDesc {
            name,
            extent: vk::Extent2D { width, height },
            format,
            usage: vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC,
            mip_levels,
            aspect: vk::ImageAspectFlags::COLOR,
        })?;

        let device = ctx.device();
        ctx.immediate_submit(|cmd| {
            let image = texture.raw;
            let color = vk::ImageAspectFlags::COLOR;
            image_barrier(
                device, cmd, image, color, 0..mip_levels,
                (vk::ImageLayout::UNDEFINED, vk::PipelineStageFlags2::NONE, vk::AccessFlags2::NONE),
                (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::PipelineStageFlags2::COPY, vk::AccessFlags2::TRANSFER_WRITE),
            );
            let copy = [vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(color).layer_count(1))
                .image_extent(vk::Extent3D { width, height, depth: 1 })];
            // SAFETY: staging holds the full level-0 image; image is in TRANSFER_DST.
            unsafe {
                device.cmd_copy_buffer_to_image(cmd, staging.raw(), image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &copy)
            };
            let (mut w, mut h) = (width as i32, height as i32);
            for level in 1..mip_levels {
                image_barrier(
                    device, cmd, image, color, level - 1..level,
                    (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE),
                    (vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::PipelineStageFlags2::BLIT, vk::AccessFlags2::TRANSFER_READ),
                );
                let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
                let blit = [vk::ImageBlit::default()
                    .src_subresource(vk::ImageSubresourceLayers::default().aspect_mask(color).mip_level(level - 1).layer_count(1))
                    .src_offsets([vk::Offset3D::default(), vk::Offset3D { x: w, y: h, z: 1 }])
                    .dst_subresource(vk::ImageSubresourceLayers::default().aspect_mask(color).mip_level(level).layer_count(1))
                    .dst_offsets([vk::Offset3D::default(), vk::Offset3D { x: nw, y: nh, z: 1 }])];
                // SAFETY: level-1 is TRANSFER_SRC, level is TRANSFER_DST.
                unsafe {
                    device.cmd_blit_image(
                        cmd, image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL, &blit, vk::Filter::LINEAR,
                    )
                };
                (w, h) = (nw, nh);
            }
            // All levels but the last are TRANSFER_SRC now; the last is TRANSFER_DST.
            if mip_levels > 1 {
                image_barrier(
                    device, cmd, image, color, 0..mip_levels - 1,
                    (vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::PipelineStageFlags2::BLIT, vk::AccessFlags2::TRANSFER_READ),
                    (vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::PipelineStageFlags2::FRAGMENT_SHADER, vk::AccessFlags2::SHADER_SAMPLED_READ),
                );
            }
            image_barrier(
                device, cmd, image, color, mip_levels - 1..mip_levels,
                (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::PipelineStageFlags2::TRANSFER, vk::AccessFlags2::TRANSFER_WRITE),
                (vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::PipelineStageFlags2::FRAGMENT_SHADER, vk::AccessFlags2::SHADER_SAMPLED_READ),
            );
        })?;
        Ok(texture)
    }

    /// Vulkan image.
    pub fn raw(&self) -> vk::Image {
        self.raw
    }

    /// View covering all mips.
    pub fn view(&self) -> vk::ImageView {
        self.view
    }

    /// Size.
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    /// Format.
    pub fn format(&self) -> vk::Format {
        self.format
    }

    /// Mip level count.
    pub fn mip_levels(&self) -> u32 {
        self.mip_levels
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        // SAFETY: owners guarantee the GPU no longer uses the image.
        unsafe {
            self.ctx.device().destroy_image_view(self.view, None);
            self.ctx.device().destroy_image(self.raw, None);
        }
        if let Some(a) = self.allocation.take() {
            if let Err(e) = self.ctx.allocator().free(a) {
                tracing::error!("failed to free image memory: {e}");
            }
        }
    }
}
