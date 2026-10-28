# gpu-compute

GPU platform services shared by `compute-core`, `raster-core` and optional math
adapters. The historical package name is retained for compatibility.

- `GpuContext`: device/queue creation and shared handles. Cloning a context
  shares the existing device; create one at the application boundary.
- Backend/capability reports and workgroup-size heuristics.
- Byte packing, legacy blocking readback and buffer binding helpers.
- Optional `cuda` driver adapter, dynamically loaded without a build-time toolkit.

Native wgpu backend features are selected here. Workspace dependencies pin the
wgpu/naga versions used by compute and raster. Domain algorithms, render state
and automatic CPU/GPU placement remain in the higher-level crates.

[Architecture and migration](../../docs/design/gpu-library-architecture.md)
describes module ownership, compatibility boundaries and remaining work.

`GpuBuffer` and `GpuBufferView` validate context ownership, range, usage and storage
binding alignment. Context identity survives cloning and distinguishes devices
created by separate wgpu instances. Context handles are immutable; field reads
remain available, while struct literals and replacing handles are unsupported.

`ByteReadback` centralizes mapping, cancellation, consumption and timeout errors.
Typed decoding and texture row layouts belong to higher-level crates.
`GpuContext::features` is adapter support; `enabled_features()` is what shaders
may actually use on the device.

`GpuContext::with_features(required)` creates a device with explicit optional
features and returns `UnsupportedFeatures` if the adapter lacks any of them.
For example, native subgroup profiling requests
`wgpu::Features::SUBGROUP | wgpu::Features::TIMESTAMP_QUERY`. The default
constructor still enables no optional features. Subgroups are native-only in
the pinned wgpu 30 API; enabling them does not make a shader browser-portable.

## Optional GPU timing

`GpuContext::with_timestamps()` explicitly requests `TIMESTAMP_QUERY`. It returns
an error when the adapter lacks the feature; the default `GpuContext::new()`
still enables no optional features. `GpuTimer` measures the interval between
the beginning and end of a compute pass using device timestamps.

```rust,no_run
use gpu_compute::{GpuContext, GpuTimer};
use std::time::Duration;

let context = GpuContext::with_timestamps()?;
let timer = GpuTimer::new(&context)?;
let mut encoder = context.device.create_command_encoder(&Default::default());
let mut timing = timer.record_compute(&mut encoder, "compute chain", |pass| {
    // Record kernels or a prepared compute-core program into this pass.
})?;
timing.submitted(context.queue.submit([timer.finish(encoder)?]));
let measured = timing.wait(Duration::from_secs(30))?;
println!("GPU pass: {} ns", measured.elapsed_ns);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Each ticket owns a separate query set. Call `submitted` after submitting its
encoder, then poll or wait. Resolution starts in a second submission after a
completion callback confirms the measured work finished. This keeps samples
correct when a timer is reused or several timed passes share one encoder.
Tickets support cancellation at either pending stage, and `wait_mut` keeps
the ticket available after a completion or mapping timeout. Equal ticks report
zero at the device's timing resolution; unavailable, backwards or wrapped
counters report an error. Absolute ticks have no wall-clock meaning.
Raw encoders and resources must belong to the timer's context. wgpu
can defer command validation until encoder finish; `timer.finish` captures
those errors. Discard the encoder and its tickets after any validation error.

The `profile_compute` example reports GPU pass time separately from CPU wall
time for encoding, submission and readback. Wall time includes profiler query
allocation and the extra completion/resolve submission; transfers outside the
pass are excluded from the GPU interval. Neither unavailable timestamps nor
failed readback fall back to CPU timing.

### Metal ordering regression

With wgpu 30.0.1, recording query resolution immediately after the measured pass
on Metal returned previous samples when query storage was reused. A one-dispatch
workload reported 4.432 ms despite its enclosing wall interval being 0.528 ms.
Fresh query storage alone mostly returned zero. The profiler therefore retains
each query set and waits for producer completion before encoding resolution.
The regression test alternates 1 and 128 dispatches in both orders, checks the
enclosing wall bound, and reads mixed-duration tickets from one encoder in
reverse order. Failed baseline outputs are retained in `benchmarks/`.
