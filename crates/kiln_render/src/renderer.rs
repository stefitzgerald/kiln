//! The forward renderer: owns GPU copies of assets and draws a [`RenderScene`].

use std::collections::{HashMap, HashSet};

use ash::vk;
use kiln_asset::{AssetServer, ColorSpace, Handle, Image, Material, Mesh};
use kiln_math::{Aabb, Affine3A, Containment, Frustum, Mat4, Vec3};
use kiln_rhi::{AdapterInfo, MemoryReport, PresentMode, Validation, ValidationStats};
use kiln_rhi_vulkan::util::image_barrier;
use kiln_rhi_vulkan::{
    AcquiredImage, Buffer, BufferDesc, ContextDesc, GpuContext, MemoryLocation, Swapchain, Texture,
    TextureDesc, VkError,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::RenderError;
use crate::gpu_types::{FrameUniforms, MaterialUniforms, ObjectUniforms, Vertex, vec4};
use crate::pipeline::{DEPTH_FORMAT, Pipelines};

/// Frames the CPU may record ahead of the GPU.
pub const FRAMES_IN_FLIGHT: usize = 2;

const OFFSCREEN_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
const MATERIAL_SETS_PER_POOL: u32 = 256;
const INITIAL_OBJECT_CAPACITY: u64 = 256;

/// Renderer configuration.
#[derive(Debug, Clone)]
pub struct RendererSettings {
    /// Application name reported to the driver.
    pub app_name: String,
    /// Presentation mode for windowed rendering.
    pub present_mode: PresentMode,
    /// Validation layer policy.
    pub validation: Validation,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            app_name: "Kiln".into(),
            present_mode: PresentMode::Fifo,
            validation: Validation::Auto,
        }
    }
}

/// One mesh to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawItem {
    /// Geometry.
    pub mesh: Handle<Mesh>,
    /// Surface.
    pub material: Handle<Material>,
    /// Model-to-world matrix.
    pub transform: Mat4,
}

/// A directional light as seen by the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLightData {
    /// Direction the light travels, in world space.
    pub direction: Vec3,
    /// Linear RGB color × intensity.
    pub color: [f32; 3],
}

/// Everything needed to render one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderScene {
    /// `projection * view` (reverse-Z).
    pub view_projection: Mat4,
    /// Camera position in world space.
    pub camera_position: Vec3,
    /// Linear RGBA clear color.
    pub clear_color: [f32; 4],
    /// Linear ambient light (color × intensity).
    pub ambient: [f32; 3],
    /// Optional sun light.
    pub light: Option<DirectionalLightData>,
    /// Meshes to draw.
    pub draws: Vec<DrawItem>,
}

impl Default for RenderScene {
    fn default() -> Self {
        Self {
            view_projection: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            ambient: [0.0; 3],
            light: None,
            draws: Vec::new(),
        }
    }
}

/// Counters from the most recent frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderStats {
    /// Draw calls issued.
    pub draws: u32,
    /// Draws skipped by frustum culling.
    pub culled: u32,
    /// Triangles submitted.
    pub triangles: u64,
}

/// Outcome of [`Renderer::render`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    /// The frame was submitted.
    Rendered,
    /// Nothing was drawn (minimized window or swapchain being recreated).
    Skipped,
}

enum Target {
    Window {
        swapchain: Swapchain,
        dirty: bool,
        requested: vk::Extent2D,
    },
    Offscreen {
        color: Texture,
        readback: Buffer,
    },
}

struct FrameData {
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    image_available: vk::Semaphore,
    in_flight: vk::Fence,
    frame_ubo: Buffer,
    object_ubo: Buffer,
    object_capacity: u64,
    set: vk::DescriptorSet,
}

struct GpuMesh {
    vertices: Buffer,
    indices: Buffer,
    index_count: u32,
    aabb: Option<Aabb>,
}

struct GpuMaterial {
    set: vk::DescriptorSet,
    double_sided: bool,
    _uniforms: Buffer,
}

struct PreparedDraw {
    double_sided: bool,
    material_set: vk::DescriptorSet,
    mesh: Handle<Mesh>,
    transform: Mat4,
}

/// Vulkan forward renderer drawing into a window or an offscreen image.
///
/// GPU copies of meshes, materials and images are created on first use from the
/// [`AssetServer`] and cached by handle. Assets are treated as immutable once uploaded.
pub struct Renderer {
    frames: Vec<FrameData>,
    frame_index: usize,
    target: Target,
    depth: Texture,
    meshes: HashMap<Handle<Mesh>, GpuMesh>,
    textures: HashMap<Handle<Image>, Texture>,
    materials: HashMap<Handle<Material>, GpuMaterial>,
    default_material: Option<GpuMaterial>,
    reported_missing: HashSet<(u8, u32, u32)>,
    white: Texture,
    sampler: vk::Sampler,
    frame_pool: vk::DescriptorPool,
    material_pools: Vec<vk::DescriptorPool>,
    pipelines: Pipelines,
    object_stride: u64,
    stats: RenderStats,
    // Last: dropped after every resource that references the device.
    ctx: GpuContext,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("adapter", &self.ctx.adapter_info().name)
            .field("extent", &self.extent())
            .field("meshes", &self.meshes.len())
            .field("materials", &self.materials.len())
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// Create a renderer presenting to `window` (any `raw-window-handle` window, e.g. winit).
    pub fn new_windowed<W: HasWindowHandle + HasDisplayHandle>(
        window: &W,
        width: u32,
        height: u32,
        settings: &RendererSettings,
    ) -> Result<Self, RenderError> {
        let display = window
            .display_handle()
            .map_err(|e| RenderError::Window(e.to_string()))?
            .as_raw();
        let win = window
            .window_handle()
            .map_err(|e| RenderError::Window(e.to_string()))?
            .as_raw();
        let (ctx, surface) = GpuContext::new_with_window(&context_desc(settings), display, win)?;
        let requested = vk::Extent2D {
            width: width.max(1),
            height: height.max(1),
        };
        let swapchain = Swapchain::new(&ctx, surface, requested, settings.present_mode)?;
        let extent = swapchain.extent();
        let format = swapchain.format();
        Self::build(
            ctx,
            Target::Window {
                swapchain,
                dirty: false,
                requested: extent,
            },
            extent,
            format,
        )
    }

    /// Create a renderer drawing into an offscreen `width`×`height` sRGB image, readable with
    /// [`Renderer::read_pixels`]. Used by tests and tools.
    pub fn new_headless(
        width: u32,
        height: u32,
        settings: &RendererSettings,
    ) -> Result<Self, RenderError> {
        let ctx = GpuContext::new_headless(&context_desc(settings))?;
        let extent = vk::Extent2D { width, height };
        let (color, readback) = create_offscreen(&ctx, extent)?;
        Self::build(
            ctx,
            Target::Offscreen { color, readback },
            extent,
            OFFSCREEN_FORMAT,
        )
    }

    fn build(
        ctx: GpuContext,
        target: Target,
        extent: vk::Extent2D,
        format: vk::Format,
    ) -> Result<Self, RenderError> {
        let device = ctx.device();
        let pipelines = Pipelines::new(&ctx, format)?;
        let depth = create_depth(&ctx, extent)?;
        let white = Texture::from_rgba8(&ctx, "white", 1, 1, &[255; 4], true)?;
        let align = ctx.limits().min_uniform_buffer_offset_alignment.max(1);
        let object_stride = (std::mem::size_of::<ObjectUniforms>() as u64).div_ceil(align) * align;

        let anisotropy = ctx.sampler_anisotropy();
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::REPEAT)
            .address_mode_v(vk::SamplerAddressMode::REPEAT)
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            .anisotropy_enable(anisotropy)
            .max_anisotropy(if anisotropy {
                ctx.limits().max_sampler_anisotropy.min(16.0)
            } else {
                1.0
            })
            .max_lod(vk::LOD_CLAMP_NONE);
        let n = FRAMES_IN_FLIGHT as u32;
        let frame_pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: n,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                descriptor_count: n,
            },
        ];
        // SAFETY: valid device and create infos; destroyed in Drop.
        let (sampler, frame_pool) = unsafe {
            let sampler = device
                .create_sampler(&sampler_info, None)
                .map_err(vkerr("create sampler"))?;
            let pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(n)
                    .pool_sizes(&frame_pool_sizes),
                None,
            );
            match pool {
                Ok(p) => (sampler, p),
                Err(e) => {
                    device.destroy_sampler(sampler, None);
                    return Err(vkerr("create frame descriptor pool")(e));
                }
            }
        };

        let mut renderer = Self {
            frames: Vec::with_capacity(FRAMES_IN_FLIGHT),
            frame_index: 0,
            target,
            depth,
            meshes: HashMap::new(),
            textures: HashMap::new(),
            materials: HashMap::new(),
            default_material: None,
            reported_missing: HashSet::new(),
            white,
            sampler,
            frame_pool,
            material_pools: Vec::new(),
            pipelines,
            object_stride,
            stats: RenderStats::default(),
            ctx,
        };
        for i in 0..FRAMES_IN_FLIGHT {
            let frame = renderer.create_frame(i)?;
            renderer.frames.push(frame);
        }
        renderer.default_material = Some(renderer.create_material(&Material::default())?);
        let info = renderer.ctx.adapter_info();
        tracing::info!(gpu = %info.name, extent = ?extent, "renderer ready");
        Ok(renderer)
    }

    fn create_frame(&mut self, i: usize) -> Result<FrameData, RenderError> {
        let ctx = &self.ctx;
        let device = ctx.device();
        let frame_ubo = Buffer::new(
            ctx,
            &BufferDesc {
                name: "frame uniforms",
                size: std::mem::size_of::<FrameUniforms>() as u64,
                usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                location: MemoryLocation::CpuToGpu,
            },
        )?;
        let object_ubo = create_object_buffer(ctx, INITIAL_OBJECT_CAPACITY * self.object_stride)?;
        let layouts = [self.pipelines.frame_layout];
        // SAFETY: valid device, pool and create infos. On failure, objects created so far are
        // leaked only until device destruction, which is acceptable for a fatal init error.
        let frame = unsafe {
            let pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(ctx.queue_family())
                        .flags(vk::CommandPoolCreateFlags::TRANSIENT),
                    None,
                )
                .map_err(vkerr("frame command pool"))?;
            let cmd = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(vkerr("frame command buffer"))?[0];
            let image_available = device
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                .map_err(vkerr("frame semaphore"))?;
            let in_flight = device
                .create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .map_err(vkerr("frame fence"))?;
            let set = device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.frame_pool)
                        .set_layouts(&layouts),
                )
                .map_err(vkerr("frame descriptor set"))?[0];
            FrameData {
                pool,
                cmd,
                image_available,
                in_flight,
                frame_ubo,
                object_ubo,
                object_capacity: INITIAL_OBJECT_CAPACITY,
                set,
            }
        };
        self.write_frame_set(&frame);
        tracing::trace!(frame = i, "frame resources created");
        Ok(frame)
    }

    fn write_frame_set(&self, frame: &FrameData) {
        let frame_info = [vk::DescriptorBufferInfo::default()
            .buffer(frame.frame_ubo.raw())
            .range(std::mem::size_of::<FrameUniforms>() as u64)];
        let object_info = [vk::DescriptorBufferInfo::default()
            .buffer(frame.object_ubo.raw())
            .range(std::mem::size_of::<ObjectUniforms>() as u64)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(frame.set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&frame_info),
            vk::WriteDescriptorSet::default()
                .dst_set(frame.set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                .buffer_info(&object_info),
        ];
        // SAFETY: the set is not in use by the GPU (its frame fence has been waited on).
        unsafe { self.ctx.device().update_descriptor_sets(&writes, &[]) };
    }

    /// Current render target size.
    pub fn extent(&self) -> vk::Extent2D {
        match &self.target {
            Target::Window { swapchain, .. } => swapchain.extent(),
            Target::Offscreen { color, .. } => color.extent(),
        }
    }

    /// Width / height of the render target.
    pub fn aspect(&self) -> f32 {
        let e = self.extent();
        if e.height == 0 {
            1.0
        } else {
            e.width as f32 / e.height as f32
        }
    }

    /// The selected GPU.
    pub fn adapter_info(&self) -> &AdapterInfo {
        self.ctx.adapter_info()
    }

    /// Validation message counts since creation.
    pub fn validation_stats(&self) -> ValidationStats {
        self.ctx.validation_stats()
    }

    /// GPU memory usage.
    pub fn memory_report(&self) -> MemoryReport {
        self.ctx.memory_report()
    }

    /// Counters from the most recent frame.
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// The underlying GPU context.
    pub fn gpu_context(&self) -> &GpuContext {
        &self.ctx
    }

    /// Request a new target size. Windowed renderers recreate the swapchain lazily on the next
    /// frame; a zero size pauses rendering. Headless renderers reallocate immediately.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let new = vk::Extent2D { width, height };
        match &mut self.target {
            Target::Window {
                swapchain,
                dirty,
                requested,
            } => {
                if new != swapchain.extent() || *dirty {
                    *requested = new;
                    *dirty = true;
                }
            }
            Target::Offscreen { color, .. } => {
                if new != color.extent() && width > 0 && height > 0 {
                    self.ctx.wait_idle()?;
                    let (color, readback) = create_offscreen(&self.ctx, new)?;
                    self.target = Target::Offscreen { color, readback };
                    self.depth = create_depth(&self.ctx, new)?;
                }
            }
        }
        Ok(())
    }

    /// Render `scene`, uploading any assets it references that are not yet on the GPU.
    pub fn render(
        &mut self,
        scene: &RenderScene,
        assets: &AssetServer,
    ) -> Result<FrameStatus, RenderError> {
        let ctx = self.ctx.clone();
        let device = ctx.device();
        let fi = self.frame_index;
        let fence = self.frames[fi].in_flight;
        // SAFETY: the fence belongs to this device.
        unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }
            .map_err(vkerr("wait for frame"))?;

        // Recreate the swapchain if the window changed size.
        let mut new_extent = None;
        if let Target::Window {
            swapchain,
            dirty,
            requested,
        } = &mut self.target
            && *dirty
        {
            if requested.width == 0 || requested.height == 0 {
                return Ok(FrameStatus::Skipped);
            }
            swapchain.recreate(*requested)?;
            *dirty = false;
            new_extent = Some((swapchain.extent(), swapchain.format()));
        }
        if let Some((extent, format)) = new_extent {
            self.depth = create_depth(&ctx, extent)?;
            if format != self.pipelines.color_format {
                ctx.wait_idle()?;
                self.pipelines = Pipelines::new(&ctx, format)?;
            }
        }

        // Upload assets and fill uniforms before acquiring, so failures leave no dangling
        // acquired image.
        let draws = self.prepare(scene, assets)?;

        let acquired: Option<AcquiredImage> = match &mut self.target {
            Target::Window {
                swapchain,
                dirty,
                requested,
            } => match swapchain.acquire(self.frames[fi].image_available)? {
                Some(img) => Some(img),
                None => {
                    *requested = swapchain.extent();
                    *dirty = true;
                    return Ok(FrameStatus::Skipped);
                }
            },
            Target::Offscreen { .. } => None,
        };

        self.record(fi, scene, &draws, acquired.as_ref())?;

        let frame = &self.frames[fi];
        let cmd_info = [vk::CommandBufferSubmitInfo::default().command_buffer(frame.cmd)];
        let wait = acquired.map(|_| {
            [vk::SemaphoreSubmitInfo::default()
                .semaphore(frame.image_available)
                .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)]
        });
        let signal = acquired.map(|a| {
            [vk::SemaphoreSubmitInfo::default()
                .semaphore(a.render_finished)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)]
        });
        let mut submit = vk::SubmitInfo2::default().command_buffer_infos(&cmd_info);
        if let (Some(wait), Some(signal)) = (&wait, &signal) {
            submit = submit
                .wait_semaphore_infos(wait)
                .signal_semaphore_infos(signal);
        }
        // SAFETY: the fence was waited on above and is not in use.
        unsafe { device.reset_fences(&[fence]) }.map_err(vkerr("reset frame fence"))?;
        ctx.submit(&[submit], fence)?;

        if let (
            Some(image),
            Target::Window {
                swapchain,
                dirty,
                requested,
            },
        ) = (acquired, &mut self.target)
            && swapchain.present(&image)?
        {
            *requested = swapchain.extent();
            *dirty = true;
        }
        self.frame_index = (fi + 1) % FRAMES_IN_FLIGHT;
        Ok(FrameStatus::Rendered)
    }

    /// Wait for rendering to finish and return the offscreen image (headless renderers only).
    pub fn read_pixels(&mut self) -> Result<Image, RenderError> {
        let Target::Offscreen { color, readback } = &self.target else {
            return Err(RenderError::NotHeadless);
        };
        self.ctx.wait_idle()?;
        let extent = color.extent();
        let data = readback.mapped().ok_or(RenderError::NotHeadless)?.to_vec();
        Ok(Image {
            width: extent.width,
            height: extent.height,
            data,
            color_space: ColorSpace::Srgb,
        })
    }

    fn prepare(
        &mut self,
        scene: &RenderScene,
        assets: &AssetServer,
    ) -> Result<Vec<PreparedDraw>, RenderError> {
        let frustum = Frustum::from_view_projection(&scene.view_projection);
        let mut stats = RenderStats::default();
        let mut draws = Vec::with_capacity(scene.draws.len());
        for d in &scene.draws {
            if !self.ensure_mesh(d.mesh, assets)? {
                continue;
            }
            let mesh = &self.meshes[&d.mesh];
            if let Some(aabb) = mesh.aabb {
                let world = aabb.transformed(&Affine3A::from_mat4(d.transform));
                if frustum.classify_aabb(&world) == Containment::Outside {
                    stats.culled += 1;
                    continue;
                }
            }
            stats.triangles += u64::from(mesh.index_count / 3);
            let (material_set, double_sided) = self.ensure_material(d.material, assets)?;
            draws.push(PreparedDraw {
                double_sided,
                material_set,
                mesh: d.mesh,
                transform: d.transform,
            });
        }
        // Minimize state changes: group by pipeline, then material, then mesh.
        draws.sort_by_key(|d| (d.double_sided, vk::Handle::as_raw(d.material_set), d.mesh));
        stats.draws = draws.len() as u32;
        self.stats = stats;

        // Per-frame uniforms. The frame's fence was waited on, so its buffers are free.
        let fi = self.frame_index;
        let needed = draws.len() as u64;
        if needed > self.frames[fi].object_capacity {
            let capacity = needed.next_power_of_two();
            self.frames[fi].object_ubo =
                create_object_buffer(&self.ctx, capacity * self.object_stride)?;
            self.frames[fi].object_capacity = capacity;
            self.write_frame_set(&self.frames[fi]);
        }
        let frame = &mut self.frames[fi];
        let (light_dir, light_color) = match scene.light {
            Some(l) => (
                vec4(l.direction.normalize_or(Vec3::NEG_Y), 0.0),
                [l.color[0], l.color[1], l.color[2], 0.0],
            ),
            None => ([0.0, -1.0, 0.0, 0.0], [0.0; 4]),
        };
        let uniforms = FrameUniforms {
            view_proj: scene.view_projection.to_cols_array_2d(),
            camera_pos: vec4(scene.camera_position, 1.0),
            light_dir,
            light_color,
            ambient: [scene.ambient[0], scene.ambient[1], scene.ambient[2], 0.0],
        };
        frame.frame_ubo.write(0, bytemuck::bytes_of(&uniforms))?;
        let stride = self.object_stride;
        let mapped = frame
            .object_ubo
            .mapped_mut()
            .ok_or_else(|| RenderError::Internal("object buffer not mapped".into()))?;
        for (i, d) in draws.iter().enumerate() {
            let offset = i * stride as usize;
            let uniforms = ObjectUniforms::new(d.transform);
            let bytes = bytemuck::bytes_of(&uniforms);
            mapped[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        Ok(draws)
    }

    fn warn_missing(&mut self, kind: u8, index: u32, generation: u32, what: &str) {
        if self.reported_missing.insert((kind, index, generation)) {
            tracing::warn!("{what} {index}v{generation} is not loaded; skipping");
        }
    }

    /// Upload the mesh if needed. `Ok(false)` if it cannot be drawn.
    fn ensure_mesh(
        &mut self,
        handle: Handle<Mesh>,
        assets: &AssetServer,
    ) -> Result<bool, RenderError> {
        if self.meshes.contains_key(&handle) {
            return Ok(true);
        }
        let Some(mesh) = assets.meshes.get(handle) else {
            self.warn_missing(0, handle.index(), handle.generation(), "mesh");
            return Ok(false);
        };
        if let Err(e) = mesh.validate() {
            if self
                .reported_missing
                .insert((1, handle.index(), handle.generation()))
            {
                tracing::warn!("mesh {handle:?} is invalid ({e}); skipping");
            }
            return Ok(false);
        }
        if mesh.indices.is_empty() {
            return Ok(false);
        }
        let vertices = Vertex::interleave(mesh);
        let gpu = GpuMesh {
            vertices: Buffer::with_data(
                &self.ctx,
                "vertices",
                vk::BufferUsageFlags::VERTEX_BUFFER,
                bytemuck::cast_slice(&vertices),
            )?,
            indices: Buffer::with_data(
                &self.ctx,
                "indices",
                vk::BufferUsageFlags::INDEX_BUFFER,
                bytemuck::cast_slice(&mesh.indices),
            )?,
            index_count: mesh.indices.len() as u32,
            aabb: mesh.aabb(),
        };
        self.meshes.insert(handle, gpu);
        Ok(true)
    }

    fn ensure_texture(
        &mut self,
        handle: Handle<Image>,
        assets: &AssetServer,
    ) -> Result<vk::ImageView, RenderError> {
        if let Some(t) = self.textures.get(&handle) {
            return Ok(t.view());
        }
        let Some(image) = assets.images.get(handle).filter(|i| i.is_valid()) else {
            self.warn_missing(2, handle.index(), handle.generation(), "image");
            return Ok(self.white.view());
        };
        let srgb = image.color_space == ColorSpace::Srgb;
        let texture = Texture::from_rgba8(
            &self.ctx,
            "texture",
            image.width,
            image.height,
            &image.data,
            srgb,
        )?;
        let view = texture.view();
        self.textures.insert(handle, texture);
        Ok(view)
    }

    fn ensure_material(
        &mut self,
        handle: Handle<Material>,
        assets: &AssetServer,
    ) -> Result<(vk::DescriptorSet, bool), RenderError> {
        if let Some(m) = self.materials.get(&handle) {
            return Ok((m.set, m.double_sided));
        }
        let Some(material) = assets.materials.get(handle).cloned() else {
            self.warn_missing(3, handle.index(), handle.generation(), "material");
            let d = self
                .default_material
                .as_ref()
                .ok_or_else(|| RenderError::Internal("no default material".into()))?;
            return Ok((d.set, d.double_sided));
        };
        let view = match material.base_color_texture {
            Some(t) => self.ensure_texture(t, assets)?,
            None => self.white.view(),
        };
        let gpu = self.create_material_with_view(&material, view)?;
        let result = (gpu.set, gpu.double_sided);
        self.materials.insert(handle, gpu);
        Ok(result)
    }

    fn create_material(&mut self, material: &Material) -> Result<GpuMaterial, RenderError> {
        let view = self.white.view();
        self.create_material_with_view(material, view)
    }

    fn create_material_with_view(
        &mut self,
        material: &Material,
        view: vk::ImageView,
    ) -> Result<GpuMaterial, RenderError> {
        let mut uniforms = Buffer::new(
            &self.ctx,
            &BufferDesc {
                name: "material uniforms",
                size: std::mem::size_of::<MaterialUniforms>() as u64,
                usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                location: MemoryLocation::CpuToGpu,
            },
        )?;
        let data = MaterialUniforms {
            base_color: material.base_color,
            flags: [if material.unlit { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        uniforms.write(0, bytemuck::bytes_of(&data))?;
        let set = self.allocate_material_set()?;
        let image_info = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let sampler_info = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(uniforms.raw())
            .range(std::mem::size_of::<MaterialUniforms>() as u64)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&image_info),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(&sampler_info),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&buffer_info),
        ];
        // SAFETY: freshly allocated set, not yet used by the GPU.
        unsafe { self.ctx.device().update_descriptor_sets(&writes, &[]) };
        Ok(GpuMaterial {
            set,
            double_sided: material.double_sided,
            _uniforms: uniforms,
        })
    }

    fn allocate_material_set(&mut self) -> Result<vk::DescriptorSet, RenderError> {
        let device = self.ctx.device();
        let layouts = [self.pipelines.material_layout];
        if let Some(&pool) = self.material_pools.last() {
            // SAFETY: valid pool and layout.
            match unsafe {
                device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pool)
                        .set_layouts(&layouts),
                )
            } {
                Ok(sets) => return Ok(sets[0]),
                Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL) => {}
                Err(e) => return Err(vkerr("allocate material set")(e)),
            }
        }
        let n = MATERIAL_SETS_PER_POOL;
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: n,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: n,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: n,
            },
        ];
        // SAFETY: valid device; the pool is destroyed in Drop.
        let pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(n)
                    .pool_sizes(&sizes),
                None,
            )
        }
        .map_err(vkerr("create material descriptor pool"))?;
        self.material_pools.push(pool);
        // SAFETY: fresh pool with capacity.
        unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )
        }
        .map(|s| s[0])
        .map_err(vkerr("allocate material set"))
    }

    fn record(
        &self,
        fi: usize,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        acquired: Option<&AcquiredImage>,
    ) -> Result<(), RenderError> {
        let device = self.ctx.device();
        let frame = &self.frames[fi];
        let cmd = frame.cmd;
        let extent = self.extent();
        let (color_image, color_view) = match (&self.target, acquired) {
            (Target::Window { .. }, Some(a)) => (a.image, a.view),
            (Target::Offscreen { color, .. }, _) => (color.raw(), color.view()),
            (Target::Window { .. }, None) => {
                return Err(RenderError::Internal("no swapchain image".into()));
            }
        };
        let color = vk::ImageAspectFlags::COLOR;
        let depth = vk::ImageAspectFlags::DEPTH;
        // SAFETY: the frame's fence was waited on, so its pool and command buffer are idle; all
        // referenced resources outlive the submission (they are owned by `self`).
        unsafe {
            device
                .reset_command_pool(frame.pool, vk::CommandPoolResetFlags::empty())
                .map_err(vkerr("reset command pool"))?;
            device
                .begin_command_buffer(
                    cmd,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(vkerr("begin frame commands"))?;

            // Previous contents are discarded. The source scope covers the previous frame's
            // attachment writes and (offscreen) readback copy of the shared images.
            image_barrier(
                device,
                cmd,
                color_image,
                color,
                0..1,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags2::COPY,
                    vk::AccessFlags2::NONE,
                ),
                (
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                    vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                ),
            );
            let depth_tests = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS;
            image_barrier(
                device,
                cmd,
                self.depth.raw(),
                depth,
                0..1,
                (
                    vk::ImageLayout::UNDEFINED,
                    depth_tests,
                    vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
                (
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                    depth_tests,
                    vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ,
                ),
            );

            let color_attachment = [vk::RenderingAttachmentInfo::default()
                .image_view(color_view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: scene.clear_color,
                    },
                })];
            let depth_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(self.depth.view())
                .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                // Reverse-Z: clear to the far plane (0).
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 0.0,
                        stencil: 0,
                    },
                });
            let area = vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent,
            };
            device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(area)
                    .layer_count(1)
                    .color_attachments(&color_attachment)
                    .depth_attachment(&depth_attachment),
            );
            // Negative height flips Y so clip space is Y-up (see ADR 0003).
            let viewport = vk::Viewport {
                x: 0.0,
                y: extent.height as f32,
                width: extent.width as f32,
                height: -(extent.height as f32),
                min_depth: 0.0,
                max_depth: 1.0,
            };
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[area]);

            let mut bound_pipeline = vk::Pipeline::null();
            let mut bound_material = vk::DescriptorSet::null();
            let mut bound_mesh = None;
            for (i, d) in draws.iter().enumerate() {
                let Some(mesh) = self.meshes.get(&d.mesh) else {
                    continue;
                };
                let pipeline = if d.double_sided {
                    self.pipelines.double_sided
                } else {
                    self.pipelines.opaque
                };
                if pipeline != bound_pipeline {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    bound_pipeline = pipeline;
                }
                let offset = (i as u64 * self.object_stride) as u32;
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipelines.layout,
                    0,
                    &[frame.set],
                    &[offset],
                );
                if d.material_set != bound_material {
                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.pipelines.layout,
                        1,
                        &[d.material_set],
                        &[],
                    );
                    bound_material = d.material_set;
                }
                if bound_mesh != Some(d.mesh) {
                    device.cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertices.raw()], &[0]);
                    device.cmd_bind_index_buffer(cmd, mesh.indices.raw(), 0, vk::IndexType::UINT32);
                    bound_mesh = Some(d.mesh);
                }
                device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
            }
            device.cmd_end_rendering(cmd);

            match &self.target {
                Target::Window { .. } => image_barrier(
                    device,
                    cmd,
                    color_image,
                    color,
                    0..1,
                    (
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                        vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                    ),
                    (
                        vk::ImageLayout::PRESENT_SRC_KHR,
                        vk::PipelineStageFlags2::NONE,
                        vk::AccessFlags2::NONE,
                    ),
                ),
                Target::Offscreen { readback, .. } => {
                    image_barrier(
                        device,
                        cmd,
                        color_image,
                        color,
                        0..1,
                        (
                            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                            vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                            vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                        ),
                        (
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk::PipelineStageFlags2::COPY,
                            vk::AccessFlags2::TRANSFER_READ,
                        ),
                    );
                    let region = [vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(color)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D {
                            width: extent.width,
                            height: extent.height,
                            depth: 1,
                        })];
                    device.cmd_copy_image_to_buffer(
                        cmd,
                        color_image,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        readback.raw(),
                        &region,
                    );
                    // Make the copy visible to host reads after the fence.
                    let barrier = [vk::MemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::COPY)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::HOST)
                        .dst_access_mask(vk::AccessFlags2::HOST_READ)];
                    device.cmd_pipeline_barrier2(
                        cmd,
                        &vk::DependencyInfo::default().memory_barriers(&barrier),
                    );
                }
            }
            device
                .end_command_buffer(cmd)
                .map_err(vkerr("end frame commands"))?;
        }
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Err(e) = self.ctx.wait_idle() {
            tracing::error!("GPU did not idle before renderer shutdown: {e}");
        }
        let device = self.ctx.device();
        // SAFETY: the GPU is idle; these objects are owned exclusively by the renderer.
        // RAII resources (buffers, textures, swapchain, pipelines) drop after this body.
        unsafe {
            for f in &self.frames {
                device.destroy_command_pool(f.pool, None);
                device.destroy_semaphore(f.image_available, None);
                device.destroy_fence(f.in_flight, None);
            }
            for &p in &self.material_pools {
                device.destroy_descriptor_pool(p, None);
            }
            device.destroy_descriptor_pool(self.frame_pool, None);
            device.destroy_sampler(self.sampler, None);
        }
    }
}

fn vkerr(context: &'static str) -> impl Fn(vk::Result) -> RenderError {
    move |result| RenderError::Vulkan(VkError::Api { context, result })
}

fn context_desc(settings: &RendererSettings) -> ContextDesc {
    ContextDesc {
        app_name: settings.app_name.clone(),
        validation: settings.validation,
        ..Default::default()
    }
}

fn create_depth(ctx: &GpuContext, extent: vk::Extent2D) -> Result<Texture, RenderError> {
    Ok(Texture::new(
        ctx,
        &TextureDesc {
            name: "depth",
            extent,
            format: DEPTH_FORMAT,
            usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            mip_levels: 1,
            aspect: vk::ImageAspectFlags::DEPTH,
        },
    )?)
}

fn create_offscreen(
    ctx: &GpuContext,
    extent: vk::Extent2D,
) -> Result<(Texture, Buffer), RenderError> {
    let color = Texture::new(
        ctx,
        &TextureDesc {
            name: "offscreen color",
            extent,
            format: OFFSCREEN_FORMAT,
            usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            mip_levels: 1,
            aspect: vk::ImageAspectFlags::COLOR,
        },
    )?;
    let readback = Buffer::new(
        ctx,
        &BufferDesc {
            name: "offscreen readback",
            size: u64::from(extent.width) * u64::from(extent.height) * 4,
            usage: vk::BufferUsageFlags::TRANSFER_DST,
            location: MemoryLocation::GpuToCpu,
        },
    )?;
    Ok((color, readback))
}

fn create_object_buffer(ctx: &GpuContext, size: u64) -> Result<Buffer, RenderError> {
    Ok(Buffer::new(
        ctx,
        &BufferDesc {
            name: "object uniforms",
            size,
            usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
            location: MemoryLocation::CpuToGpu,
        },
    )?)
}
