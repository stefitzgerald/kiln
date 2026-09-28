//! GPU tests for the Vulkan backend (TC-GPU-01). Ignored by default; run with
//! `cargo xtask gpu-test` (or `cargo test -p kiln_rhi_vulkan -- --ignored`).

#![allow(clippy::unwrap_used, clippy::chunks_exact_to_as_chunks)]

use kiln_rhi::Validation;
use kiln_rhi_vulkan::{Buffer, BufferDesc, ContextDesc, GpuContext, MemoryLocation, Texture, vk};

fn context() -> GpuContext {
    let validation = if std::env::var_os("CI").is_some() {
        Validation::Required
    } else {
        Validation::Enabled
    };
    GpuContext::new_headless(&ContextDesc {
        validation,
        ..Default::default()
    })
    .expect("failed to create a headless Vulkan context")
}

fn assert_clean(ctx: &GpuContext) {
    let stats = ctx.validation_stats();
    assert_eq!(stats.errors, 0, "validation errors: {stats:?}");
    assert_eq!(stats.warnings, 0, "validation warnings: {stats:?}");
}

/// TC-GPU-01: headless instance + device creation.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn tc_gpu_01_headless_context() {
    let ctx = context();
    let info = ctx.adapter_info();
    println!(
        "GPU: {} ({:?}), Vulkan {:?}, driver {}",
        info.name, info.device_type, info.api_version, info.driver
    );
    assert!(!info.name.is_empty());
    assert!(info.api_version >= (1, 3, 0));
    if std::env::var_os("CI").is_some() {
        assert!(
            ctx.validation_stats().enabled,
            "CI requires validation layers"
        );
    }
    ctx.wait_idle().unwrap();
    assert_clean(&ctx);
}

#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn buffer_upload_roundtrip() {
    let ctx = context();
    let data: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 251) as u8).collect();
    let device_local =
        Buffer::with_data(&ctx, "src", vk::BufferUsageFlags::TRANSFER_SRC, &data).unwrap();
    let readback = Buffer::new(
        &ctx,
        &BufferDesc {
            name: "readback",
            size: data.len() as u64,
            usage: vk::BufferUsageFlags::TRANSFER_DST,
            location: MemoryLocation::GpuToCpu,
        },
    )
    .unwrap();
    // SAFETY: both buffers are live and at least `data.len()` bytes long.
    ctx.immediate_submit(|cmd| unsafe {
        ctx.device().cmd_copy_buffer(
            cmd,
            device_local.raw(),
            readback.raw(),
            &[vk::BufferCopy::default().size(data.len() as u64)],
        );
    })
    .unwrap();
    assert_eq!(readback.mapped().unwrap(), &data[..]);
    drop((device_local, readback));
    assert_eq!(
        ctx.memory_report().allocated_bytes,
        0,
        "all memory freed on drop"
    );
    assert_clean(&ctx);
}

#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn texture_upload_with_mips() {
    let ctx = context();
    let pixels = vec![255u8; 256 * 128 * 4];
    let tex = Texture::from_rgba8(&ctx, "test", 256, 128, &pixels, true).unwrap();
    assert_eq!(
        tex.extent(),
        vk::Extent2D {
            width: 256,
            height: 128
        }
    );
    assert!(
        tex.mip_levels() == 9 || tex.mip_levels() == 1,
        "{}",
        tex.mip_levels()
    );
    assert!(Texture::from_rgba8(&ctx, "bad", 4, 4, &[0; 3], true).is_err());
    drop(tex);
    assert_clean(&ctx);
}

#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn invalid_arguments_are_errors() {
    let ctx = context();
    let err = Buffer::new(
        &ctx,
        &BufferDesc {
            name: "empty",
            size: 0,
            usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
            location: MemoryLocation::CpuToGpu,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("size 0"));
    let mut small = Buffer::new(
        &ctx,
        &BufferDesc {
            name: "small",
            size: 16,
            usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
            location: MemoryLocation::CpuToGpu,
        },
    )
    .unwrap();
    assert!(small.write(8, &[0; 16]).is_err());
    small.write(0, &[1; 16]).unwrap();
    assert_clean(&ctx);
}

/// Proves the validation counter used by every GPU test actually detects errors.
#[test]
#[ignore = "requires a Vulkan 1.3 GPU; run `cargo xtask gpu-test`"]
fn validation_counter_detects_errors() {
    let ctx = context();
    if !ctx.validation_stats().enabled {
        eprintln!("validation layer not installed; skipping");
        return;
    }
    // mipLodBias far beyond maxSamplerLodBias violates VUID-VkSamplerCreateInfo-mipLodBias-01069.
    // Drivers simply clamp it, so this is safe to execute.
    let info = vk::SamplerCreateInfo::default().mip_lod_bias(1.0e6);
    // SAFETY: valid device; the invalid bias is clamped by drivers.
    let sampler = unsafe { ctx.device().create_sampler(&info, None) }.unwrap();
    // SAFETY: the sampler was never used.
    unsafe { ctx.device().destroy_sampler(sampler, None) };
    assert!(
        ctx.validation_stats().errors >= 1,
        "{:?}",
        ctx.validation_stats()
    );
}
