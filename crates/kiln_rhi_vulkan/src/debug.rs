use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

use ash::vk;

/// Counters shared with the debug messenger callback. Boxed so its address is stable.
#[derive(Debug, Default)]
pub(crate) struct DebugState {
    pub(crate) errors: AtomicU32,
    pub(crate) warnings: AtomicU32,
}

impl DebugState {
    pub(crate) fn errors(&self) -> u32 {
        self.errors.load(Ordering::Relaxed)
    }
    pub(crate) fn warnings(&self) -> u32 {
        self.warnings.load(Ordering::Relaxed)
    }
}

/// Messenger create info that routes messages to `tracing` and counts validation issues.
pub(crate) fn messenger_info(state: &DebugState) -> vk::DebugUtilsMessengerCreateInfoEXT<'static> {
    vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(callback))
        .user_data(state as *const DebugState as *mut c_void)
}

unsafe extern "system" fn callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut c_void,
) -> vk::Bool32 {
    // SAFETY: Vulkan passes a valid callback-data pointer (or null) for the call's duration,
    // and `user` is the `DebugState` registered with the messenger, which outlives it.
    let (message, id, state) = unsafe {
        let message = data
            .as_ref()
            .filter(|d| !d.p_message.is_null())
            .map(|d| CStr::from_ptr(d.p_message).to_string_lossy())
            .unwrap_or_default();
        let id = data
            .as_ref()
            .filter(|d| !d.p_message_id_name.is_null())
            .map(|d| CStr::from_ptr(d.p_message_id_name).to_string_lossy())
            .unwrap_or_default();
        (message, id, (user as *const DebugState).as_ref())
    };
    let validation = types.contains(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION);
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        if validation {
            if let Some(s) = state {
                s.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        tracing::error!(target: "vulkan", id = %id, "{message}");
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        if validation {
            if let Some(s) = state {
                s.warnings.fetch_add(1, Ordering::Relaxed);
            }
        }
        tracing::warn!(target: "vulkan", id = %id, "{message}");
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::INFO) {
        tracing::debug!(target: "vulkan", "{message}");
    } else {
        tracing::trace!(target: "vulkan", "{message}");
    }
    vk::FALSE
}
