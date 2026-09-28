//! `cargo xtask doctor`: environment diagnostics with actionable fixes.

use std::path::PathBuf;
use std::process::{Command, Stdio};

enum Level {
    Ok,
    Warn,
    Fail,
}

struct Report {
    failures: usize,
}

impl Report {
    fn line(&mut self, level: Level, what: &str, detail: &str) {
        let tag = match level {
            Level::Ok => "[ ok ]",
            Level::Warn => "[warn]",
            Level::Fail => {
                self.failures += 1;
                "[FAIL]"
            }
        };
        println!("{tag} {what}: {detail}");
    }
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn run() -> Result<(), String> {
    let mut r = Report { failures: 0 };
    println!("Kiln environment check\n");

    // Rust toolchain.
    let host = match output("rustc", &["-vV"]) {
        Some(v) => {
            let version = v.lines().next().unwrap_or_default().to_owned();
            let host = v
                .lines()
                .find_map(|l| l.strip_prefix("host: "))
                .unwrap_or("unknown")
                .to_owned();
            r.line(Level::Ok, "rustc", &format!("{version} ({host})"));
            host
        }
        None => {
            r.line(Level::Fail, "rustc", "not found. Install from https://rustup.rs");
            String::new()
        }
    };

    // Linker: actually link a trivial program; that is the only reliable check.
    let dir = std::env::temp_dir().join("kiln-doctor");
    let _ = std::fs::create_dir_all(&dir);
    let src = dir.join("probe.rs");
    let exe = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let linked = std::fs::write(&src, "fn main() {}\n").is_ok()
        && Command::new("rustc")
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    if linked {
        r.line(Level::Ok, "linker", "can build and link executables");
    } else if host.ends_with("windows-msvc") {
        r.line(
            Level::Fail,
            "linker",
            "MSVC link.exe not usable. Install Visual Studio Build Tools with the C++ workload, \
             or switch to the GNU toolchain: `rustup default stable-x86_64-pc-windows-gnu` \
             (requires MinGW-w64 gcc on PATH). Note: MSYS2's coreutils `link` shadowing \
             MSVC's is a common cause.",
        );
    } else if host.ends_with("windows-gnu") {
        r.line(Level::Fail, "linker", "gcc/ld not usable. Install MinGW-w64 (e.g. MSYS2 `mingw-w64-x86_64-gcc`) and add it to PATH");
    } else {
        r.line(Level::Fail, "linker", "cannot link a test program. Install a C toolchain (build-essential / Xcode CLT)");
    }

    // Vulkan SDK (optional for building; needed for validation layers).
    match std::env::var_os("VULKAN_SDK").map(PathBuf::from) {
        Some(sdk) if sdk.exists() => r.line(Level::Ok, "VULKAN_SDK", &sdk.display().to_string()),
        Some(sdk) => r.line(Level::Warn, "VULKAN_SDK", &format!("{} does not exist", sdk.display())),
        None => r.line(
            Level::Warn,
            "VULKAN_SDK",
            "not set. Building works without it, but validation layers need the SDK \
             (https://vulkan.lunarg.com) or your distro's vulkan-validationlayers package",
        ),
    }

    // Validation layer manifest.
    let mut layer_dirs: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("VK_LAYER_PATH") {
        layer_dirs.extend(std::env::split_paths(&p));
    }
    if let Some(sdk) = std::env::var_os("VULKAN_SDK").map(PathBuf::from) {
        layer_dirs.push(sdk.join("Bin"));
        layer_dirs.push(sdk.join("share/vulkan/explicit_layer.d"));
        layer_dirs.push(sdk.join("etc/vulkan/explicit_layer.d"));
    }
    layer_dirs.push("/usr/share/vulkan/explicit_layer.d".into());
    layer_dirs.push("/usr/local/share/vulkan/explicit_layer.d".into());
    match layer_dirs.iter().map(|d| d.join("VkLayer_khronos_validation.json")).find(|p| p.exists()) {
        Some(p) => r.line(Level::Ok, "validation layer", &p.display().to_string()),
        None => r.line(
            Level::Warn,
            "validation layer",
            "VK_LAYER_KHRONOS_validation not found; GPU tests will run without validation",
        ),
    }

    // Vulkan loader + devices.
    let vulkaninfo = output("vulkaninfo", &["--summary"]);
    match vulkaninfo {
        Some(text) => {
            let devices: Vec<String> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("deviceName").map(|s| s.trim_start_matches([' ', '=']).trim().to_owned()))
                .collect();
            let versions: Vec<String> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("apiVersion").map(|s| s.trim_start_matches([' ', '=']).trim().to_owned()))
                .collect();
            if devices.is_empty() {
                r.line(Level::Fail, "GPU", "Vulkan loader works but reports no devices");
            }
            for (name, ver) in devices.iter().zip(versions.iter()) {
                let ok = ver.split('.').take(2).map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
                let supports_13 = ok.first().copied().unwrap_or(0) > 1 || (ok.first() == Some(&1) && ok.get(1).copied().unwrap_or(0) >= 3);
                r.line(
                    if supports_13 { Level::Ok } else { Level::Warn },
                    "GPU",
                    &format!("{name} (Vulkan {ver}){}", if supports_13 { "" } else { " - Kiln requires Vulkan 1.3" }),
                );
            }
        }
        None => r.line(
            Level::Warn,
            "Vulkan",
            "`vulkaninfo` not found or failed. Install GPU drivers with Vulkan 1.3 support \
             (or mesa-vulkan-drivers for the lavapipe software rasterizer)",
        ),
    }

    // Optional tools.
    if output("cargo-deny", &["--version"]).is_some() {
        r.line(Level::Ok, "cargo-deny", "installed");
    } else {
        r.line(Level::Warn, "cargo-deny", "not installed (`cargo install cargo-deny --locked`); `xtask ci` will skip license checks");
    }

    println!();
    if r.failures == 0 {
        println!("Environment looks good.");
        Ok(())
    } else {
        Err(format!("{} required check(s) failed", r.failures))
    }
}
