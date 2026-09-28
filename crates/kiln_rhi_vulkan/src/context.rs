use std::ffi::{CStr, CString, c_char};
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, MutexGuard};

use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use kiln_rhi::{AdapterInfo, DeviceType, MemoryReport, Validation, ValidationStats};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::debug::{DebugState, messenger_info};
use crate::swapchain::Surface;
use crate::{ResultExt, VkError, VkResult};

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";
const PORTABILITY_SUBSET: &CStr = c"VK_KHR_portability_subset";

/// Options for [`GpuContext`] creation.
#[derive(Debug, Clone)]
pub struct ContextDesc {
    /// Application name reported to the driver.
    pub app_name: String,
    /// Validation layer policy.
    pub validation: Validation,
    /// Prefer a GPU whose name contains this (case-insensitive). Defaults to `KILN_GPU`.
    pub preferred_gpu: Option<String>,
}

impl Default for ContextDesc {
    fn default() -> Self {
        Self {
            app_name: "Kiln".into(),
            validation: Validation::Auto,
            preferred_gpu: std::env::var("KILN_GPU").ok().filter(|s| !s.is_empty()),
        }
    }
}

struct UploadContext {
    pool: vk::CommandPool,
    fence: vk::Fence,
}

/// Instance-level objects, destroyed manually on construction failure.
struct InstanceParts {
    entry: ash::Entry,
    instance: ash::Instance,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    debug_state: Box<DebugState>,
    surface_fn: ash::khr::surface::Instance,
}

impl InstanceParts {
    /// # Safety
    /// No child objects of the instance may remain.
    unsafe fn destroy(&self) {
        // SAFETY: guaranteed by the caller.
        unsafe {
            if let Some((loader, messenger)) = &self.debug {
                loader.destroy_debug_utils_messenger(*messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

pub(crate) struct ContextInner {
    parts: InstanceParts,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) device: ash::Device,
    pub(crate) swapchain_fn: Option<ash::khr::swapchain::Device>,
    pub(crate) queue: vk::Queue,
    pub(crate) queue_family: u32,
    queue_lock: Mutex<()>,
    allocator: ManuallyDrop<Mutex<Allocator>>,
    upload: Mutex<UploadContext>,
    pub(crate) limits: vk::PhysicalDeviceLimits,
    pub(crate) sampler_anisotropy: bool,
    adapter: AdapterInfo,
    validation_enabled: bool,
}

impl Drop for ContextInner {
    fn drop(&mut self) {
        // SAFETY: this is the last reference to the device; every resource holds an Arc to
        // the context, so all of them have already been destroyed.
        unsafe {
            let _ = self.device.device_wait_idle();
            let upload = self.upload.get_mut().unwrap_or_else(|p| p.into_inner());
            self.device.destroy_command_pool(upload.pool, None);
            self.device.destroy_fence(upload.fence, None);
            ManuallyDrop::drop(&mut self.allocator);
            self.device.destroy_device(None);
            self.parts.destroy();
        }
        tracing::debug!("Vulkan context destroyed");
    }
}

/// Shared handle to the Vulkan instance, device, queue and allocator. Cheap to clone.
#[derive(Clone)]
pub struct GpuContext(pub(crate) Arc<ContextInner>);

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter", &self.0.adapter.name)
            .finish_non_exhaustive()
    }
}

impl GpuContext {
    /// Create a context without presentation support (offscreen rendering, tests).
    pub fn new_headless(desc: &ContextDesc) -> VkResult<Self> {
        Self::create(desc, None).map(|(ctx, _)| ctx)
    }

    /// Create a context that can present to the given window, returning its surface.
    pub fn new_with_window(
        desc: &ContextDesc,
        display: RawDisplayHandle,
        window: RawWindowHandle,
    ) -> VkResult<(Self, Surface)> {
        let (ctx, surface) = Self::create(desc, Some((display, window)))?;
        let surface = surface.ok_or_else(|| VkError::InvalidArgument("surface missing".into()))?;
        Ok((ctx, surface))
    }

    fn create(
        desc: &ContextDesc,
        window: Option<(RawDisplayHandle, RawWindowHandle)>,
    ) -> VkResult<(Self, Option<Surface>)> {
        let parts = create_instance(desc, window.map(|w| w.0))?;
        let raw_surface = match window {
            // SAFETY: the handles come from a live window that outlives the surface (the
            // caller owns both and drops the surface first).
            Some((display, win)) => match unsafe {
                ash_window::create_surface(&parts.entry, &parts.instance, display, win, None)
            } {
                Ok(s) => Some(s),
                Err(e) => {
                    // SAFETY: nothing else was created from the instance.
                    unsafe { parts.destroy() };
                    return Err(VkError::Api {
                        context: "create window surface",
                        result: e,
                    });
                }
            },
            None => None,
        };
        match create_device(desc, &parts, raw_surface) {
            Ok(dev) => {
                let ctx = GpuContext(Arc::new(ContextInner {
                    validation_enabled: parts.debug.is_some(),
                    parts,
                    physical_device: dev.physical_device,
                    swapchain_fn: dev.swapchain_fn,
                    queue: dev.queue,
                    queue_family: dev.queue_family,
                    queue_lock: Mutex::new(()),
                    allocator: ManuallyDrop::new(Mutex::new(dev.allocator)),
                    upload: Mutex::new(dev.upload),
                    limits: dev.limits,
                    sampler_anisotropy: dev.sampler_anisotropy,
                    adapter: dev.adapter,
                    device: dev.device,
                }));
                let surface = raw_surface.map(|raw| Surface::from_raw(ctx.clone(), raw));
                Ok((ctx, surface))
            }
            Err(e) => {
                // SAFETY: the device was not created; only the surface depends on the instance.
                unsafe {
                    if let Some(s) = raw_surface {
                        parts.surface_fn.destroy_surface(s, None);
                    }
                    parts.destroy();
                }
                Err(e)
            }
        }
    }

    /// The logical device.
    pub fn device(&self) -> &ash::Device {
        &self.0.device
    }

    /// The Vulkan instance.
    pub fn instance(&self) -> &ash::Instance {
        &self.0.parts.instance
    }

    /// The physical device.
    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.0.physical_device
    }

    /// Surface extension functions.
    pub fn surface_fn(&self) -> &ash::khr::surface::Instance {
        &self.0.parts.surface_fn
    }

    /// Graphics (and present, when windowed) queue family index.
    pub fn queue_family(&self) -> u32 {
        self.0.queue_family
    }

    /// Device limits.
    pub fn limits(&self) -> &vk::PhysicalDeviceLimits {
        &self.0.limits
    }

    /// `true` if anisotropic filtering was enabled.
    pub fn sampler_anisotropy(&self) -> bool {
        self.0.sampler_anisotropy
    }

    /// Information about the selected GPU.
    pub fn adapter_info(&self) -> &AdapterInfo {
        &self.0.adapter
    }

    /// Validation message counters.
    pub fn validation_stats(&self) -> ValidationStats {
        ValidationStats {
            enabled: self.0.validation_enabled,
            errors: self.0.parts.debug_state.errors(),
            warnings: self.0.parts.debug_state.warnings(),
        }
    }

    /// GPU memory usage.
    pub fn memory_report(&self) -> MemoryReport {
        let report = self.allocator().generate_report();
        MemoryReport {
            allocated_bytes: report.total_allocated_bytes,
            reserved_bytes: report.total_capacity_bytes,
            allocation_count: report.allocations.len(),
        }
    }

    /// Lock the memory allocator.
    pub fn allocator(&self) -> MutexGuard<'_, Allocator> {
        self.0.allocator.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Submit to the queue. `vkQueueSubmit2` requires external synchronization, which this
    /// provides.
    pub fn submit(&self, submits: &[vk::SubmitInfo2<'_>], fence: vk::Fence) -> VkResult<()> {
        let _guard = self.0.queue_lock.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: the queue is externally synchronized by `queue_lock`.
        unsafe { self.0.device.queue_submit2(self.0.queue, submits, fence) }.ctx("queue submit")
    }

    /// Present on the queue. Returns `true` if the swapchain is suboptimal.
    pub(crate) fn present(&self, info: &vk::PresentInfoKHR<'_>) -> Result<bool, vk::Result> {
        let swapchain_fn = self
            .0
            .swapchain_fn
            .as_ref()
            .ok_or(vk::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let _guard = self.0.queue_lock.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: the queue is externally synchronized by `queue_lock`.
        unsafe { swapchain_fn.queue_present(self.0.queue, info) }
    }

    /// Record commands with `record`, submit them and wait for completion. Intended for
    /// uploads and other one-off work, not per-frame rendering.
    pub fn immediate_submit(&self, record: impl FnOnce(vk::CommandBuffer)) -> VkResult<()> {
        let upload = self.0.upload.lock().unwrap_or_else(|p| p.into_inner());
        let device = &self.0.device;
        // SAFETY: the pool and fence are owned by this context and guarded by `upload`.
        unsafe {
            let cmd = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(upload.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .ctx("allocate upload command buffer")?[0];
            let result = (|| {
                device
                    .begin_command_buffer(
                        cmd,
                        &vk::CommandBufferBeginInfo::default()
                            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                    )
                    .ctx("begin upload commands")?;
                record(cmd);
                device.end_command_buffer(cmd).ctx("end upload commands")?;
                device
                    .reset_fences(&[upload.fence])
                    .ctx("reset upload fence")?;
                let cmd_info = [vk::CommandBufferSubmitInfo::default().command_buffer(cmd)];
                self.submit(
                    &[vk::SubmitInfo2::default().command_buffer_infos(&cmd_info)],
                    upload.fence,
                )?;
                device
                    .wait_for_fences(&[upload.fence], true, u64::MAX)
                    .ctx("wait for upload")
            })();
            device.free_command_buffers(upload.pool, &[cmd]);
            result
        }
    }

    /// Block until the GPU is idle.
    pub fn wait_idle(&self) -> VkResult<()> {
        let _guard = self.0.queue_lock.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: the queue lock prevents concurrent submission.
        unsafe { self.0.device.device_wait_idle() }.ctx("device wait idle")
    }
}

fn create_instance(
    desc: &ContextDesc,
    display: Option<RawDisplayHandle>,
) -> VkResult<InstanceParts> {
    // SAFETY: loading the system Vulkan loader; its initialization has no preconditions.
    let entry = unsafe { ash::Entry::load() }.map_err(|e| VkError::Loading(e.to_string()))?;

    // SAFETY: plain query.
    let version = unsafe { entry.try_enumerate_instance_version() }
        .ctx("query instance version")?
        .unwrap_or(vk::API_VERSION_1_0);
    let (major, minor) = (
        vk::api_version_major(version),
        vk::api_version_minor(version),
    );
    if (major, minor) < (1, 3) {
        return Err(VkError::UnsupportedVersion(major, minor));
    }

    // SAFETY: plain queries.
    let layers = unsafe { entry.enumerate_instance_layer_properties() }.ctx("enumerate layers")?;
    // SAFETY: plain query.
    let extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
        .ctx("enumerate instance extensions")?;
    let has_layer = layers
        .iter()
        .any(|l| l.layer_name_as_c_str() == Ok(VALIDATION_LAYER));
    let has_ext = |name: &CStr| {
        extensions
            .iter()
            .any(|e| e.extension_name_as_c_str() == Ok(name))
    };

    let validation = match desc.validation.resolve() {
        Validation::Required if !has_layer => return Err(VkError::ValidationUnavailable),
        Validation::Required | Validation::Enabled if has_layer => true,
        Validation::Enabled => {
            tracing::warn!("validation requested but VK_LAYER_KHRONOS_validation is not installed");
            false
        }
        _ => false,
    };
    let debug_utils = validation && has_ext(ash::ext::debug_utils::NAME);

    let mut ext_names: Vec<*const c_char> = Vec::new();
    if let Some(display) = display {
        ext_names.extend_from_slice(
            ash_window::enumerate_required_extensions(display).ctx("query surface extensions")?,
        );
    }
    if debug_utils {
        ext_names.push(ash::ext::debug_utils::NAME.as_ptr());
    }
    let mut flags = vk::InstanceCreateFlags::empty();
    if has_ext(ash::khr::portability_enumeration::NAME) {
        ext_names.push(ash::khr::portability_enumeration::NAME.as_ptr());
        flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
    }
    let layer_names: Vec<*const c_char> = if validation {
        vec![VALIDATION_LAYER.as_ptr()]
    } else {
        Vec::new()
    };

    let app_name = CString::new(desc.app_name.replace('\0', "")).unwrap_or_default();
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .engine_name(c"Kiln")
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_3);

    let debug_state = Box::<DebugState>::default();
    let mut messenger_ci = messenger_info(&debug_state);
    let mut create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_layer_names(&layer_names)
        .enabled_extension_names(&ext_names)
        .flags(flags);
    if debug_utils {
        // Also capture messages from instance creation/destruction.
        create_info = create_info.push_next(&mut messenger_ci);
    }
    // SAFETY: all pointers in `create_info` reference locals that outlive the call.
    let instance = unsafe { entry.create_instance(&create_info, None) }.ctx("create instance")?;

    let debug = if debug_utils {
        let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
        // SAFETY: `debug_state` is boxed and stored alongside the messenger, outliving it.
        match unsafe { loader.create_debug_utils_messenger(&messenger_info(&debug_state), None) } {
            Ok(m) => Some((loader, m)),
            Err(e) => {
                tracing::warn!("failed to create debug messenger: {e}");
                None
            }
        }
    } else {
        None
    };
    let validation_active = debug.is_some();
    tracing::info!(
        validation = validation_active,
        "Vulkan {major}.{minor} instance created"
    );
    let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);
    Ok(InstanceParts {
        entry,
        instance,
        debug,
        debug_state,
        surface_fn,
    })
}

struct DeviceParts {
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    swapchain_fn: Option<ash::khr::swapchain::Device>,
    queue: vk::Queue,
    queue_family: u32,
    allocator: Allocator,
    upload: UploadContext,
    limits: vk::PhysicalDeviceLimits,
    sampler_anisotropy: bool,
    adapter: AdapterInfo,
}

struct Candidate {
    physical_device: vk::PhysicalDevice,
    queue_family: u32,
    score: u32,
    info: AdapterInfo,
    anisotropy: bool,
    portability_subset: bool,
    limits: vk::PhysicalDeviceLimits,
}

fn create_device(
    desc: &ContextDesc,
    parts: &InstanceParts,
    surface: Option<vk::SurfaceKHR>,
) -> VkResult<DeviceParts> {
    let instance = &parts.instance;
    // SAFETY: plain query.
    let devices = unsafe { instance.enumerate_physical_devices() }.ctx("enumerate GPUs")?;
    let mut rejected = Vec::new();
    let mut best: Option<Candidate> = None;
    for pd in devices {
        match evaluate(parts, pd, surface, desc.preferred_gpu.as_deref()) {
            Ok(c) => {
                tracing::debug!(gpu = %c.info.name, score = c.score, "GPU candidate");
                if best.as_ref().is_none_or(|b| c.score > b.score) {
                    best = Some(c);
                }
            }
            Err(reason) => rejected.push(reason),
        }
    }
    let Some(c) = best else {
        return Err(VkError::NoSuitableDevice(if rejected.is_empty() {
            "no Vulkan devices reported".into()
        } else {
            rejected.join("; ")
        }));
    };

    let priorities = [1.0f32];
    let queue_info = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(c.queue_family)
        .queue_priorities(&priorities)];
    let mut ext_names: Vec<*const c_char> = Vec::new();
    if surface.is_some() {
        ext_names.push(ash::khr::swapchain::NAME.as_ptr());
    }
    if c.portability_subset {
        ext_names.push(PORTABILITY_SUBSET.as_ptr());
    }
    let mut features13 = vk::PhysicalDeviceVulkan13Features::default()
        .dynamic_rendering(true)
        .synchronization2(true);
    let mut features = vk::PhysicalDeviceFeatures2::default()
        .features(vk::PhysicalDeviceFeatures::default().sampler_anisotropy(c.anisotropy))
        .push_next(&mut features13);
    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&ext_names)
        .push_next(&mut features);
    // SAFETY: the physical device and all create-info pointers are valid for the call.
    let device = unsafe { instance.create_device(c.physical_device, &create_info, None) }
        .ctx("create device")?;
    // SAFETY: family/index were requested in `queue_info`.
    let queue = unsafe { device.get_device_queue(c.queue_family, 0) };

    let destroy_device = |device: &ash::Device| {
        // SAFETY: nothing else has been created from the device yet on these error paths.
        unsafe { device.destroy_device(None) }
    };
    let allocator = match Allocator::new(&AllocatorCreateDesc {
        instance: instance.clone(),
        device: device.clone(),
        physical_device: c.physical_device,
        debug_settings: Default::default(),
        buffer_device_address: false,
        allocation_sizes: Default::default(),
    }) {
        Ok(a) => a,
        Err(e) => {
            destroy_device(&device);
            return Err(VkError::Allocation(e.to_string()));
        }
    };
    // SAFETY: valid device; objects are destroyed in ContextInner::drop.
    let upload = unsafe {
        let pool = device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(c.queue_family)
                .flags(vk::CommandPoolCreateFlags::TRANSIENT),
            None,
        );
        let fence = device.create_fence(&vk::FenceCreateInfo::default(), None);
        match (pool, fence) {
            (Ok(pool), Ok(fence)) => UploadContext { pool, fence },
            (pool, fence) => {
                if let Ok(p) = pool {
                    device.destroy_command_pool(p, None);
                }
                if let Ok(f) = fence {
                    device.destroy_fence(f, None);
                }
                drop(allocator);
                destroy_device(&device);
                return Err(VkError::Api {
                    context: "create upload objects",
                    result: vk::Result::ERROR_INITIALIZATION_FAILED,
                });
            }
        }
    };
    let swapchain_fn = surface.map(|_| ash::khr::swapchain::Device::new(instance, &device));
    tracing::info!(
        gpu = %c.info.name,
        kind = ?c.info.device_type,
        api = ?c.info.api_version,
        driver = %c.info.driver,
        "selected GPU"
    );
    Ok(DeviceParts {
        physical_device: c.physical_device,
        device,
        swapchain_fn,
        queue,
        queue_family: c.queue_family,
        allocator,
        upload,
        limits: c.limits,
        sampler_anisotropy: c.anisotropy,
        adapter: c.info,
    })
}

fn evaluate(
    parts: &InstanceParts,
    pd: vk::PhysicalDevice,
    surface: Option<vk::SurfaceKHR>,
    preferred: Option<&str>,
) -> Result<Candidate, String> {
    let instance = &parts.instance;
    let mut driver = vk::PhysicalDeviceDriverProperties::default();
    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
    // SAFETY: plain queries on a valid physical device.
    unsafe { instance.get_physical_device_properties2(pd, &mut props2) };
    let props = props2.properties;
    let name = props
        .device_name_as_c_str()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let v = props.api_version;
    let api_version = (
        vk::api_version_major(v),
        vk::api_version_minor(v),
        vk::api_version_patch(v),
    );
    if (api_version.0, api_version.1) < (1, 3) {
        return Err(format!(
            "{name}: supports Vulkan {}.{} only",
            api_version.0, api_version.1
        ));
    }

    let mut f13 = vk::PhysicalDeviceVulkan13Features::default();
    let mut f2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut f13);
    // SAFETY: plain query.
    unsafe { instance.get_physical_device_features2(pd, &mut f2) };
    let anisotropy = f2.features.sampler_anisotropy == vk::TRUE;
    if f13.dynamic_rendering != vk::TRUE || f13.synchronization2 != vk::TRUE {
        return Err(format!(
            "{name}: missing dynamicRendering or synchronization2"
        ));
    }

    // SAFETY: plain query.
    let exts = unsafe { instance.enumerate_device_extension_properties(pd) }
        .map_err(|e| format!("{name}: {e}"))?;
    let has_ext = |n: &CStr| exts.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
    if surface.is_some() && !has_ext(ash::khr::swapchain::NAME) {
        return Err(format!("{name}: no VK_KHR_swapchain"));
    }

    // SAFETY: plain query.
    let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
    let queue_family = families
        .iter()
        .enumerate()
        .filter(|(_, f)| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .map(|(i, _)| i as u32)
        .find(|&i| match surface {
            // SAFETY: plain query with a valid surface.
            Some(s) => unsafe {
                parts
                    .surface_fn
                    .get_physical_device_surface_support(pd, i, s)
            }
            .unwrap_or(false),
            None => true,
        })
        .ok_or_else(|| format!("{name}: no graphics queue that can present"))?;

    let device_type = match props.device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => DeviceType::Discrete,
        vk::PhysicalDeviceType::INTEGRATED_GPU => DeviceType::Integrated,
        vk::PhysicalDeviceType::CPU => DeviceType::Cpu,
        vk::PhysicalDeviceType::VIRTUAL_GPU => DeviceType::Virtual,
        _ => DeviceType::Other,
    };
    let mut score = match device_type {
        DeviceType::Discrete => 1000,
        DeviceType::Integrated => 500,
        DeviceType::Virtual => 100,
        DeviceType::Cpu => 10,
        DeviceType::Other => 1,
    };
    if preferred.is_some_and(|p| name.to_lowercase().contains(&p.to_lowercase())) {
        score += 100_000;
    }
    let driver_name = driver
        .driver_name_as_c_str()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let driver_info = driver
        .driver_info_as_c_str()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Candidate {
        physical_device: pd,
        queue_family,
        score,
        info: AdapterInfo {
            name,
            device_type,
            vendor_id: props.vendor_id,
            api_version,
            driver: format!("{driver_name} {driver_info}").trim().to_owned(),
        },
        anisotropy,
        portability_subset: has_ext(PORTABILITY_SUBSET),
        limits: props.limits,
    })
}
