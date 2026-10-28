# osv-math

Published name for the workspace `math-core` leaf: small dense `f64` vector
helpers (`V2` / `V3`), point-cloud kernels with deterministic CPU references,
camera/viewport math and a shared typed `Error` / `Result`.

Rust crate name remains `math_core` (`use math_core::…`). Builds on **stable** Rust
and has no runtime dependencies.

```toml
[dependencies]
math-core = { package = "osv-math", version = "0.1" }
```

## Device acceleration

`*_accelerated` functions take an `Acceleration` placement. This crate is
CPU-only: device kernels are reached through the `DeviceKernels` port, which
`osv-math-compute` (wgpu, CUDA, tensor/MLX) implements and installs with
`math_compute::install()`. Without an installed backend, or when a device
kernel declines, the CPU reference runs.
