# MCP bin benchmark

Recorded on 2026-11-17. These measurements cover one small bin workflow. They
do not prove the assembly scan target or viewer frame-time target.

## Machine and method

- Windows x86-64; Intel Core i7-11800H, 8 cores and 16 logical processors.
- Available graphics devices: Intel UHD Graphics and NVIDIA GeForce RTX 3050
  Laptop GPU. The benchmark did not record which adapter was selected.
- Rust 1.99.0. Debug build; dependency optimisation follows Cargo.toml.
- One process per MCP session. OS caches were not cleared. No visible window.
- The fixture is copied into a temporary project below `.cargo`. The checked-in
  bin is not changed. Each JSON report contains its full source recipe.
- Times include stdio request/reply, JSON, transcript writes and timing output.
  Renders also include GPU setup when needed, PNG writing and base64 transfer.
  New-process open excludes process startup and the MCP handshake.

## GPU reuse result

Before GPU reuse, every render created a device and pipeline. After the change,
headless saves reuse one GPU per process. A mutex covers upload, draw and
readback because the camera uniform is shared. Failed device creation can be
retried on a later call. GUI rendering retains its own device.

| Measure | Before | After |
|---|---:|---:|
| Section render median, 5 calls, 640 x 480 | 374.27 ms | 23.58 ms |
| First section render | 521.30 ms | 540.63 ms |
| Later section render range | 346.24–385.96 ms | 22.19–23.80 ms |

The reduction applies to repeated headless renders in this process and fixture.
It does not remove first-render setup or establish a general CAD speed-up.
The after-run parameter-edit median was 28.76 ms, status median 0.90 ms, and
new-process open median 24.42 ms. These operations were not optimised here.

Raw samples:

- [Before GPU reuse](hex-bin-debug.json)
- [After GPU reuse](hex-bin-debug-gpu-cache.json)
- [After spline support](hex-bin-debug-spline.json)
- [After initial face history](hex-bin-debug-face-history.json)

The spline-support run uses the same bin fixture and debug profile. Median
parameter edit time is 32.32 ms and median section render time is 28.84 ms;
the first render is 684.80 ms. This fixture has no splines. The run records
current behaviour and does not isolate a speed change or measure spline cost.

The initial-face-history run records a parameter-edit median of 38.90 ms, section
render median of 41.28 ms and new-process open median of 41.82 ms. First render
is 786.27 ms. These samples are higher than the earlier run; they are not a
controlled estimate of history overhead. The fixture and debug profile are the
same. Raw samples include the timing variation.

## Repeat

```powershell
cargo build --offline --workspace --all-targets
target/debug/examples/benchmark_mcp.exe docs/benchmarks/new-run.json
```

Use the binary from the same build profile for the server and client. Report
machine, profile, fixture and sample counts with any comparison. The benchmark
asserts the model volume and changed parameter results while it runs.

The render regression tests compare pixel output for selected parts and for
concurrent requests with different cameras. Wall-clock limits are not test
assertions because other machine activity can change them.
