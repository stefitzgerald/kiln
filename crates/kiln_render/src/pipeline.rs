//! Descriptor layouts and graphics pipelines for the mesh pass.

use std::io::Cursor;

use ash::vk;
use kiln_rhi_vulkan::{GpuContext, VkError, util};

use crate::RenderError;
use crate::gpu_types::Vertex;

const MESH_VS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesh.vs_main.spv"));
const MESH_FS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesh.fs_main.spv"));

pub(crate) const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// Descriptor set layouts, pipeline layout and the pipeline variants for one color format.
pub(crate) struct Pipelines {
    ctx: GpuContext,
    pub(crate) frame_layout: vk::DescriptorSetLayout,
    pub(crate) material_layout: vk::DescriptorSetLayout,
    pub(crate) layout: vk::PipelineLayout,
    /// Back-face culled.
    pub(crate) opaque: vk::Pipeline,
    /// No culling.
    pub(crate) double_sided: vk::Pipeline,
    pub(crate) color_format: vk::Format,
}

impl Pipelines {
    pub(crate) fn new(ctx: &GpuContext, color_format: vk::Format) -> Result<Self, RenderError> {
        let device = ctx.device();
        let vk_err = |context: &'static str| move |result| RenderError::Vulkan(VkError::Api { context, result });

        let frame_bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX),
        ];
        let material_bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];

        // Build incrementally; `Drop` cleans up whatever was created if a later step fails.
        let mut this = Self {
            ctx: ctx.clone(),
            frame_layout: vk::DescriptorSetLayout::null(),
            material_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            opaque: vk::Pipeline::null(),
            double_sided: vk::Pipeline::null(),
            color_format,
        };
        // SAFETY: valid device and create infos; objects are destroyed in Drop.
        unsafe {
            this.frame_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&frame_bindings),
                    None,
                )
                .map_err(vk_err("frame set layout"))?;
            this.material_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&material_bindings),
                    None,
                )
                .map_err(vk_err("material set layout"))?;
            let set_layouts = [this.frame_layout, this.material_layout];
            this.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                    None,
                )
                .map_err(vk_err("pipeline layout"))?;
        }

        let vs_words = ash::util::read_spv(&mut Cursor::new(MESH_VS))
            .map_err(|e| RenderError::Shader(format!("mesh.vs_main: {e}")))?;
        let fs_words = ash::util::read_spv(&mut Cursor::new(MESH_FS))
            .map_err(|e| RenderError::Shader(format!("mesh.fs_main: {e}")))?;
        let vs = util::create_shader_module(device, &vs_words).map_err(vk_err("vertex shader"))?;
        let fs = match util::create_shader_module(device, &fs_words) {
            Ok(fs) => fs,
            Err(e) => {
                // SAFETY: unused module.
                unsafe { device.destroy_shader_module(vs, None) };
                return Err(vk_err("fragment shader")(e));
            }
        };
        let opaque = this.create_pipeline(vs, fs, vk::CullModeFlags::BACK);
        let double_sided = this.create_pipeline(vs, fs, vk::CullModeFlags::NONE);
        // SAFETY: modules are no longer needed once pipelines exist.
        unsafe {
            device.destroy_shader_module(vs, None);
            device.destroy_shader_module(fs, None);
        }
        this.opaque = opaque.map_err(vk_err("opaque pipeline"))?;
        this.double_sided = double_sided.map_err(vk_err("double-sided pipeline"))?;
        Ok(this)
    }

    fn create_pipeline(
        &self,
        vs: vk::ShaderModule,
        fs: vk::ShaderModule,
        cull: vk::CullModeFlags,
    ) -> Result<vk::Pipeline, vk::Result> {
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vs)
                .name(c"vs_main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fs)
                .name(c"fs_main"),
        ];
        let bindings = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(Vertex::STRIDE)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let attributes = [
            (0, vk::Format::R32G32B32_SFLOAT, 0),
            (1, vk::Format::R32G32B32_SFLOAT, 12),
            (2, vk::Format::R32G32_SFLOAT, 24),
            (3, vk::Format::R32G32B32A32_SFLOAT, 32),
        ]
        .map(|(location, format, offset)| {
            vk::VertexInputAttributeDescription::default()
                .location(location)
                .binding(0)
                .format(format)
                .offset(offset)
        });
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        // With the negative-height viewport, COUNTER_CLOCKWISE matches the usual
        // "counter-clockwise in Y-up NDC is front-facing" convention (glTF, OpenGL).
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(cull)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        // Reverse-Z: nearer fragments have larger depth.
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::GREATER_OR_EQUAL);
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let color_formats = [self.color_format];
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_formats)
            .depth_attachment_format(DEPTH_FORMAT);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layout)
            .push_next(&mut rendering);
        // SAFETY: all referenced state lives until the call returns.
        unsafe {
            self.ctx
                .device()
                .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
                .map(|p| p[0])
                .map_err(|(_, e)| e)
        }
    }
}

impl Drop for Pipelines {
    fn drop(&mut self) {
        let d = self.ctx.device();
        // SAFETY: the renderer waits for the GPU to idle before dropping pipelines; null
        // handles are ignored by Vulkan.
        unsafe {
            d.destroy_pipeline(self.opaque, None);
            d.destroy_pipeline(self.double_sided, None);
            d.destroy_pipeline_layout(self.layout, None);
            d.destroy_descriptor_set_layout(self.frame_layout, None);
            d.destroy_descriptor_set_layout(self.material_layout, None);
        }
    }
}
