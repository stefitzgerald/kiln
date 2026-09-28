//! Developer tasks. Run `cargo xtask help`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod assets;
mod doctor;

const HELP: &str = "\
cargo xtask <command>

Commands:
  doctor         Check that the toolchain, linker, Vulkan SDK and GPU are set up
  ci             Run the same checks as CI: fmt, clippy, tests, docs, cargo-deny
  gpu-test       Run GPU tests (headless Vulkan; needs a GPU or lavapipe)
  bless-goldens  Re-render golden images used by GPU tests
  gen-assets     Regenerate generated test assets (tests/assets/CheckerCube.glb)
  fetch-assets   Download large sample models to assets/external/
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("doctor") => doctor::run(),
        Some("ci") => ci(),
        Some("gpu-test") => gpu_test(false, &args[1..]),
        Some("bless-goldens") => gpu_test(true, &args[1..]),
        Some("gen-assets") => assets::generate(&root()),
        Some("fetch-assets") => assets::fetch(&root()),
        Some("help" | "-h" | "--help") | None => {
            print!("{HELP}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{HELP}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nxtask failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_owned()
}

fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root());
    cmd
}

fn run_step(name: &str, mut cmd: Command) -> Result<(), String> {
    println!("\n==> {name}");
    let status = cmd.status().map_err(|e| format!("{name}: failed to start: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{name} failed ({status})")) }
}

fn ci() -> Result<(), String> {
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--check"]);
    run_step("rustfmt", fmt)?;

    let mut clippy = cargo();
    clippy.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    run_step("clippy", clippy)?;

    let mut test = cargo();
    test.args(["test", "--workspace"]);
    run_step("tests", test)?;

    let mut doc = cargo();
    doc.args(["doc", "--workspace", "--no-deps"]).env("RUSTDOCFLAGS", "-D warnings");
    run_step("docs", doc)?;

    let has_deny = Command::new("cargo-deny").arg("--version").output().is_ok_and(|o| o.status.success());
    if has_deny {
        let mut deny = cargo();
        deny.args(["deny", "check"]);
        run_step("cargo-deny", deny)?;
    } else if std::env::var_os("CI").is_some() {
        return Err("cargo-deny is required in CI".into());
    } else {
        println!("\n==> cargo-deny: skipped (install with `cargo install cargo-deny --locked`)");
    }
    println!("\nAll CI checks passed.");
    Ok(())
}

fn gpu_test(bless: bool, extra: &[String]) -> Result<(), String> {
    let mut cmd = cargo();
    cmd.args(["test", "-p", "kiln_rhi_vulkan", "-p", "kiln_render", "--", "--ignored"]);
    cmd.args(extra);
    if bless {
        cmd.env("KILN_BLESS", "1");
    }
    run_step(if bless { "bless goldens" } else { "gpu tests" }, cmd)
}
