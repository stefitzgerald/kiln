//! Small helpers for recording commands.

use std::ops::Range;

use ash::vk;

/// Layout, stage and access for one side of a barrier.
pub type BarrierState = (vk::ImageLayout, vk::PipelineStageFlags2, vk::AccessFlags2);

/// Record a `synchronization2` image layout transition for `mips` of a single-layer image.
pub fn image_barrier(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    mips: Range<u32>,
    src: BarrierState,
    dst: BarrierState,
) {
    let barrier = [vk::ImageMemoryBarrier2::default()
        .image(image)
        .old_layout(src.0)
        .src_stage_mask(src.1)
        .src_access_mask(src.2)
        .new_layout(dst.0)
        .dst_stage_mask(dst.1)
        .dst_access_mask(dst.2)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(aspect)
                .base_mip_level(mips.start)
                .level_count(mips.end - mips.start)
                .layer_count(1),
        )];
    // SAFETY: the caller records into a command buffer in the recording state.
    unsafe {
        device.cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().image_memory_barriers(&barrier))
    };
}

/// Create a shader module from SPIR-V words.
pub fn create_shader_module(device: &ash::Device, words: &[u32]) -> Result<vk::ShaderModule, vk::Result> {
    // SAFETY: `words` is valid SPIR-V produced by the build script.
    unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(words), None) }
}
