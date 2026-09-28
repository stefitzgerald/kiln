# Manual test suites (M0)

Run these on a Windows machine with a Vulkan 1.3 GPU before closing a milestone, and on
Linux/macOS when available. Record results in [`results-M0.md`](results-M0.md).

**Setup:** `cargo xtask doctor` must pass. Run examples from the repository root. Use
`--release` for the performance case (TC-MAN-08). Set `KILN_LOG=info` (default) or
`KILN_LOG=debug` to see GPU, swapchain and validation messages in the terminal.

In every case below, "no validation errors" means the terminal shows no `ERROR vulkan`
or `WARN vulkan` lines. Debug builds enable the validation layer automatically when the
Vulkan SDK is installed.

---

### TC-MAN-01: window opens and closes

1. `cargo run --example hello_window`
2. Observe the window, then press **Esc**.
3. Run it again and close it with the title bar **X**.

**Expected:** a 1280×720 window titled "Kiln" with a dark blue-gray background. Esc and
X both close it immediately. `echo $?` (bash) or `$LASTEXITCODE` (PowerShell) prints `0`.
The log shows `selected GPU`, `swapchain created` and no validation errors.

### TC-MAN-02: resize, maximize, restore

1. `cargo run --example spinning_cube`
2. Drag each window edge and corner, quickly and slowly, for about 10 seconds.
3. Maximize, then restore. Snap the window to half the screen (Win+←).

**Expected:** no crash or freeze. The cube stays centered with correct proportions (never
stretched or squashed) after each change. The log shows `swapchain created` with the new
sizes and no validation errors.

### TC-MAN-03: minimize

1. `cargo run --example spinning_cube`
2. Minimize for at least 10 seconds while watching the process in Task Manager.
3. Restore.

**Expected:** CPU usage drops to about 0–2 % while minimized, with no errors. On restore
the cube keeps spinning without a large jump in rotation.

### TC-MAN-04: clear color

`cargo run --example clear_color`

**Expected:** the whole client area is cornflower blue (sRGB 100, 149, 237; ±2 with a
color picker). It is not washed out or too dark, which confirms the sRGB swapchain.

### TC-MAN-05: triangle

`cargo run --example triangle`

**Expected:** a triangle centered in the window on black, with the **red** corner at the
top, **green** at bottom-left and **blue** at bottom-right, and smooth gradients between
them. Resizing keeps it centered and undistorted.

### TC-MAN-06: spinning cube

`cargo run --example spinning_cube`

**Expected:** an orange cube tumbling smoothly above a gray ground plane. Lighting comes
from above and to the side, so faces facing away are darker but not black. There is no
flicker or z-fighting where the cube is near the ground. The title shows
`… | NN FPS (x.xx ms)`, which is about the monitor refresh rate because v-sync (FIFO) is on.

### TC-MAN-07: glTF viewer and fly camera

1. `cargo run --example gltf_viewer -- tests/assets/CheckerCube.glb`
2. Hold the **right mouse button** and move the mouse. Release it.
3. Press **W/A/S/D**, **Q/E**, hold **Shift**, scroll the wheel.
4. Repeat step 1 with `tests/assets/Box.glb`.

**Expected:**
- The model is fully visible and framed on start.
- With the right button held, the view turns in the direction of the mouse and the cursor
  hides. Pitch stops just short of straight up or down, and the camera never flips.
- W/S move forward and back, A/D strafe, Q/E move down and up. Shift is about 4× faster.
  Scrolling up increases speed and scrolling down decreases it.
- The checker texture is crisp up close and does not shimmer at glancing angles (mipmaps
  and anisotropic filtering).
- Box.glb shows a red cube.

### TC-MAN-08: large model performance

1. `cargo xtask fetch-assets`
2. `cargo run --release --example gltf_viewer -- assets/external/DamagedHelmet.glb`
3. Maximize to 1920×1080 or larger.

**Expected:** the log's `model loaded` `load_time` is under 2 s. The helmet shows its
albedo texture (the full PBR material comes in M1). The title shows at least 60 FPS
(v-sync-limited) on a discrete GPU.

### TC-MAN-09: logging

`KILN_LOG=debug cargo run --example triangle` (PowerShell: `$env:KILN_LOG="debug"; cargo run --example triangle`)

**Expected:** the log contains the GPU name and driver, the Vulkan version, the swapchain
format (`B8G8R8A8_SRGB` or similar), the present mode (`FIFO`) and image count. There are
zero validation warnings or errors.

### TC-MAN-10: DPI and monitor change

(Requires two monitors with different scale factors, e.g. 100 % and 150 %.)

1. `cargo run --example spinning_cube`
2. Drag the window from one monitor to the other and back.

**Expected:** no crash. The swapchain is recreated (log line), and the image stays sharp
and correctly proportioned on both monitors.

### TC-MAN-11: bad input

1. `cargo run --example gltf_viewer -- does/not/exist.glb`
2. Create an invalid file (`echo hello > bad.glb`) and run the viewer on it.

**Expected:** no window opens. A one-line `error: …` names the problem (`asset not found:
does/not/exist.glb`, or `failed to parse …`), followed by a usage line. There is no panic
or backtrace, and the exit code is `1`.
