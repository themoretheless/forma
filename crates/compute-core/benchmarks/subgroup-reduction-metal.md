# Native subgroup reduction study

## Result

Keep subgroups optional and experimental. On this Apple M4 Max, subgroup addition did not consistently beat the best portable shared-memory tree. The 4M-element winner used the portable tree; at 16M elements the best subgroup median was slightly lower, but its timing variance was large. Choosing the workgroup count mattered more than replacing the final reduction primitive.

No production reduction shader or default device features changed. `GpuContext::with_features` exposes explicit feature requests; `new` and `with_timestamps` retain their existing behavior. An unsupported request returns an error.

## Reproduce

Hardware: Apple M4 Max, 40 GPU cores, Metal 4. The wgpu backend reported `metal`, subgroup size range 4–64, and a 1 ns timestamp period. The actual subgroup width was not separately probed. Dependencies: wgpu 30.0.1, Naga 30.0.1.

```sh
cargo run --offline --release --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_subgroup_reduction
```

Use `-- --validate-only` for native correctness checks without the timing sweep. The context explicitly requires `SUBGROUP | TIMESTAMP_QUERY`. The shader sources and driver are in [the example](../examples/bench_subgroup_reduction.rs); [raw output](subgroup-reduction-metal.txt) retains every sample and the result error for every configuration.

The experiment compares a portable tree, `subgroupAdd`, and an experimental shuffle butterfly. Workgroup sizes are 64, 128, and 256; per-lane accumulators are 1, 4, 8, and 16; workgroup caps are 256, 1024, 4096, and 65535. All configurations use the same coalesced input traversal and group scheduling formula. Each reduction continues through partial buffers to one scalar.

There are 576 measured configurations across four input lengths, with 17 samples each and rotated execution order. Each size receives at least 200 ms of unprofiled warmup. Host and GPU times come from separate runs:

- Host: command encoding, submission, and a four-byte scalar readback. No timestamp queries.
- GPU: all compute stages in one timestamped compute pass. Query resolution is submitted after producer completion, using the corrected profiler.
- Both exclude pipeline compilation, uploads, uniform preparation, and partial-buffer allocation.

## Measurements

The table selects the lowest GPU median within each family. Host medians belong to the same selected configuration. All times are microseconds; these are results from one sweep, not cross-device guarantees.

| Elements | Family | Workgroup / items / cap | GPU median | GPU p90 | Host median |
| ---: | --- | --- | ---: | ---: | ---: |
| 4,097 | Tree | 128 / 1 / 65535 | 23.375 | 24.250 | 151.209 |
| 4,097 | Add | 128 / 1 / 65535 | 19.666 | 20.375 | 142.292 |
| 1,000,003 | Tree | 256 / 1 / 256 | 13.917 | 14.750 | 139.666 |
| 1,000,003 | Add | 256 / 1 / 256 | 12.500 | 13.333 | 137.167 |
| 4,000,003 | Tree | 256 / 1 / 256 | 26.125 | 30.791 | 154.917 |
| 4,000,003 | Add | 64 / 1 / 1024 | 27.292 | 31.167 | 164.542 |
| 16,000,003 | Tree | 256 / 1 / 256 | 148.333 | 378.750 | 331.166 |
| 16,000,003 | Add | 256 / 1 / 4096 | 135.375 | 292.667 | 313.125 |

At 4M elements, the tree configuration with workgroup 128, eight accumulators, and cap 1024 took 46.000 µs GPU / 178.083 µs host. Reducing the schedule to workgroup 256, one accumulator, and cap 256 brought it to 26.125 / 154.917 µs without subgroup features. At 16M, the same comparison was 238.167 / 429.292 versus 148.333 / 331.166 µs. These scheduling candidates were passed to the separate portable reduction study.

The shuffle candidates also failed to establish a consistent win: the best GPU medians were 26.750 µs at 4M and 138.041 µs at 16M, with a 399.584 µs p90 at 16M. Their restricted active-lane contract makes them unsuitable as a general fallback.

## Correctness and limits

Native correctness fixtures passed for all 36 kernels, plus Add and Shuffle variants with workgroup size 16. Inputs include zero, one, odd lengths, subgroup/workgroup boundaries, signed fractions, and balanced cancellation. Outputs are prefilled before every run, checking that empty reductions overwrite stale values. These fixtures use a 1e-6 absolute error bound. The timed positive fractional inputs are checked against an f64 sum with a relative 1e-6 bound; the largest observed absolute error was 0.35449219 at a total near four million. Floating-point reduction order changes are expected.

The Add candidate has every invocation participate, uses `subgroupBroadcastFirst` to select an active writer, stores one partial per subgroup, then folds partials after a workgroup barrier. It does not assume a relationship between subgroup IDs and local invocation indices.

The shuffle butterfly is explicitly experimental: active invocation IDs must form a dense prefix beginning at zero. Guarding an inactive XOR partner does not make it correct for arbitrary sparse active masks. Passing workgroup-size-16 fixtures establishes behavior on this Metal device only. It does not establish a portable mapping guarantee.

CPU-only Naga validation covers 48 generated variants, including workgroup size 16. The feature integration test confirms both requested features on this device, and verifies that a separate default context still enables neither optional feature. Strict Clippy passed for the example and all gpu-compute targets.

## Primary API and language evidence

WGSL defines subgroup operations over active invocations and provides no defined mapping between subgroup invocations and local invocation indices. `subgroupAdd` sums active invocations; reading an inactive `subgroupShuffleXor` target yields an indeterminate value. These rules explain the Add implementation and the shuffle restriction. [W3C WGSL: subgroups](https://www.w3.org/TR/WGSL/#subgroups), [subgroup built-in functions](https://www.w3.org/TR/WGSL/#subgroup-builtin-functions).

wgpu 30 exposes `SUBGROUP` as an optional, native-only feature; support in this native Metal study does not imply browser support. [wgpu 30.0.1 feature documentation](https://docs.rs/wgpu/30.0.1/wgpu/struct.Features.html#associatedconstant.SUBGROUP).

The pinned local Naga 30.0.1 source lists `subgroups` among unimplemented enable extensions in `src/front/wgsl/parse/directive/enable_extension.rs`; its parser accepts the native subgroup built-ins directly. The candidates therefore omit `enable subgroups;`. They also use `subgroupBroadcastFirst` because this pinned parser does not recognize `subgroupElect`. CPU parser validation and native pipeline creation verify the exact source accepted by these dependency versions.
