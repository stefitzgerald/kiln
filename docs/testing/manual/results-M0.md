# M0 manual test results

Add a new run section for each test pass (newest first). Result: ✅ pass, ❌ fail (link an
issue), ⏭ skipped (reason).

## Run: 2026-09-29, stefitzgerald

- Commit: `88bc53b` (M0 + PR #1 CI fixes)
- OS: Windows 11 Pro 10.0.26200
- GPU / driver: AMD Radeon RX 9070 XT, AMD proprietary 25.20.42.14 (LLPC), Vulkan 1.4.329
- Toolchain: rustc 1.98.1, host `x86_64-pc-windows-gnu`

| ID | Result | Notes |
|---|---|---|
| TC-BLD-01 | ✅ | `cargo xtask doctor`: all checks ok |
| TC-BLD-02 | ✅ | Clean workspace build, zero warnings |
| TC-BLD-03 | ✅ | `cargo xtask ci`: all checks passed, including cargo-deny |
| TC-BLD-04 | ✅ | CI green on `main` at `88bc53b` ([run 36451856215](https://github.com/stefitzgerald/kiln/actions/runs/36451856215)): lint, tests on 3 OSes, MSRV 1.88, cargo-deny, GPU tests + windowed smoke runs on lavapipe |
| TC-MAN-01 | ✅ | |
| TC-MAN-02 | ✅ | |
| TC-MAN-03 | ✅ | |
| TC-MAN-04 | ✅ | |
| TC-MAN-05 | ✅ | |
| TC-MAN-06 | ✅ | |
| TC-MAN-07 | ✅ | |
| TC-MAN-08 | ✅ | DamagedHelmet load time 247 ms (release) |
| TC-MAN-09 | ✅ | |
| TC-MAN-10 | ✅ | |
| TC-MAN-11 | ✅ | Missing file: one-line error, exit code 1 |

## Template

```markdown
## Run: YYYY-MM-DD, <tester>

- Commit: `<git rev-parse --short HEAD>`
- OS: <e.g. Windows 11 24H2>
- GPU / driver: <from `cargo xtask doctor`>
- Toolchain: <`rustc -vV` host line>

| ID | Result | Notes |
|---|---|---|
| TC-BLD-01 | | |
| TC-BLD-02 | | |
| TC-BLD-03 | | |
| TC-BLD-04 | | |
| TC-MAN-01 | | |
| TC-MAN-02 | | |
| TC-MAN-03 | | |
| TC-MAN-04 | | |
| TC-MAN-05 | | |
| TC-MAN-06 | | |
| TC-MAN-07 | | |
| TC-MAN-08 | | |
| TC-MAN-09 | | |
| TC-MAN-10 | | |
| TC-MAN-11 | | |
```
