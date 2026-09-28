//! Compiles `shaders/*.wgsl` to SPIR-V (one module per entry point) with naga.
//! Output: `$OUT_DIR/<file stem>.<entry point>.spv`.

use std::path::Path;

use naga::back::spv;
use naga::valid::{Capabilities, ValidationFlags, Validator};

fn main() {
    println!("cargo:rerun-if-changed=shaders");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let mut entries: Vec<_> = std::fs::read_dir("shaders")
        .expect("shaders directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wgsl"))
        .collect();
    entries.sort();
    for path in entries {
        println!("cargo:rerun-if-changed={}", path.display());
        compile(&path, Path::new(&out_dir));
    }
}

fn compile(path: &Path, out_dir: &Path) {
    let name = path.display().to_string();
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{name}: {e}"));
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|e| panic!("\n{}", e.emit_to_string_with_path(&source, &name)));
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("\n{}", e.emit_to_string_with_path(&source, &name)));

    // The Vulkan backend flips Y with a negative viewport, so naga must not also flip it.
    let mut options = spv::Options::default();
    options.flags.remove(spv::WriterFlags::ADJUST_COORDINATE_SPACE);
    options.lang_version = (1, 3);

    let stem = path.file_stem().and_then(|s| s.to_str()).expect("utf-8 file name");
    for ep in &module.entry_points {
        let pipeline = spv::PipelineOptions { shader_stage: ep.stage, entry_point: ep.name.clone() };
        let words = spv::write_vec(&module, &info, &options, Some(&pipeline))
            .unwrap_or_else(|e| panic!("{name}: SPIR-V generation for `{}` failed: {e}", ep.name));
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let out = out_dir.join(format!("{stem}.{}.spv", ep.name));
        std::fs::write(&out, bytes).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    }
}
