# Direct low-storage scatter: matched WGSL measurement

Measured on 2026-09-27, Apple M4 Max (40 GPU cores), Metal. This compares
resident packed f16/BF16 scatter with explicit resident f32 expansion followed
by the existing f32 scatter and a final low cast. Both paths return the same
low output and invalid-index count. It measures the conversion-inclusive
baseline, not a generic native f32 versus f16 arithmetic advantage.

## Method

Run `cargo run --release --offline --locked --manifest-path crates/Cargo.toml
-p compute-core --example bench_tensor_low_scatter` with an otherwise idle GPU.
The [benchmark source](../examples/bench_tensor_low_scatter.rs) uses the shared
[measurement harness](../examples/support/low_bench.rs): 200 ms warmup, 31 samples
per mode, rotated path/order, reusable recorded programs and resident inputs.
GPU timestamps bracket a shared compute pass. Separate unprofiled wall timings
include command encoding, submission and identical output/count readback.
They exclude pipeline/program construction and initial upload. Exact output and
count checks run after each recorded timing, including warmup executions.

Every case uses all five operations and both dtypes. Quarter-integer Add inputs
remain exact in f32 for every fold order; Multiply uses +/-1 updates. The host
reference folds f64 values and rounds low significands with nearest-even using
numerical scaling, independently of device codecs. Full NaN payloads, subnormal
and signed-zero behavior are covered by conformance rather than inferred from
these finite-input timings. Low results and invalid counts were checked on
every execution; all 50 cases completed.

| Case | Base shape | Valid indices | Destination-axis coordinates | Layout |
| --- | --- | --- | --- | --- |
| small | 17 x 19 | 65 | 19 | contiguous |
| unique | 257 x 1025 | 1025 | 1025, unique permutation | contiguous |
| spread | 129 x 1025 | 2051 | 1025 | contiguous |
| hot | 17 x 19 | 4099 | 1 per row | contiguous |
| strided | 257 x 1025 | 513 | 1025, 513 selected | base and updates transposed |

Each case also has two invalid indices, counted once each independently of rows.
Rows have distinct destination elements, so the hot case contends on 17 elements.

## Final GPU medians

Milliseconds; ratio = cast_f32 / direct_low. Values describe this device, run
and workload. Raw logs include p90, all samples and host timings.

| Case / dtype / operation | Cast f32 ms | Direct low ms | Ratio |
| --- | ---: | ---: | ---: |
| small / F16 / Replace | 0.027792 | 0.020583 | 1.35x |
| small / F16 / Add | 0.024209 | 0.019041 | 1.27x |
| small / F16 / Multiply | 0.021708 | 0.017708 | 1.23x |
| small / F16 / Min | 0.021792 | 0.016125 | 1.35x |
| small / F16 / Max | 0.022042 | 0.016583 | 1.33x |
| small / Bf16 / Replace | 0.025000 | 0.018750 | 1.33x |
| small / Bf16 / Add | 0.022291 | 0.018041 | 1.24x |
| small / Bf16 / Multiply | 0.022084 | 0.018875 | 1.17x |
| small / Bf16 / Min | 0.023792 | 0.016667 | 1.43x |
| small / Bf16 / Max | 0.022042 | 0.016583 | 1.33x |
| unique / F16 / Replace | 0.089791 | 0.039000 | 2.30x |
| unique / F16 / Add | 0.084625 | 0.054250 | 1.56x |
| unique / F16 / Multiply | 0.083958 | 0.051834 | 1.62x |
| unique / F16 / Min | 0.086208 | 0.036625 | 2.35x |
| unique / F16 / Max | 0.084084 | 0.036000 | 2.34x |
| unique / Bf16 / Replace | 0.086125 | 0.038208 | 2.25x |
| unique / Bf16 / Add | 0.077958 | 0.049542 | 1.57x |
| unique / Bf16 / Multiply | 0.082959 | 0.052458 | 1.58x |
| unique / Bf16 / Min | 0.084833 | 0.034000 | 2.50x |
| unique / Bf16 / Max | 0.084916 | 0.034375 | 2.47x |
| spread / F16 / Replace | 0.070875 | 0.037542 | 1.89x |
| spread / F16 / Add | 0.063292 | 0.040625 | 1.56x |
| spread / F16 / Multiply | 0.065000 | 0.039875 | 1.63x |
| spread / F16 / Min | 0.065083 | 0.032875 | 1.98x |
| spread / F16 / Max | 0.062917 | 0.032041 | 1.96x |
| spread / Bf16 / Replace | 0.068541 | 0.036416 | 1.88x |
| spread / Bf16 / Add | 0.065375 | 0.039958 | 1.64x |
| spread / Bf16 / Multiply | 0.064458 | 0.040041 | 1.61x |
| spread / Bf16 / Min | 0.064750 | 0.032000 | 2.02x |
| spread / Bf16 / Max | 0.066292 | 0.032709 | 2.03x |
| hot / F16 / Replace | 0.033792 | 0.022250 | 1.52x |
| hot / F16 / Add | 4.976500 | 4.838916 | 1.03x |
| hot / F16 / Multiply | 0.057959 | 0.051334 | 1.13x |
| hot / F16 / Min | 0.040833 | 0.020833 | 1.96x |
| hot / F16 / Max | 0.048833 | 0.025542 | 1.91x |
| hot / Bf16 / Replace | 0.035042 | 0.022792 | 1.54x |
| hot / Bf16 / Add | 4.962792 | 4.848167 | 1.02x |
| hot / Bf16 / Multiply | 0.057000 | 0.049083 | 1.16x |
| hot / Bf16 / Min | 0.041041 | 0.021167 | 1.94x |
| hot / Bf16 / Max | 0.048583 | 0.025750 | 1.89x |
| strided / F16 / Replace | 0.072833 | 0.031375 | 2.32x |
| strided / F16 / Add | 0.071583 | 0.046708 | 1.53x |
| strided / F16 / Multiply | 0.071500 | 0.046083 | 1.55x |
| strided / F16 / Min | 0.070500 | 0.027500 | 2.56x |
| strided / F16 / Max | 0.067042 | 0.027166 | 2.47x |
| strided / Bf16 / Replace | 0.073334 | 0.031667 | 2.32x |
| strided / Bf16 / Add | 0.069000 | 0.044625 | 1.55x |
| strided / Bf16 / Multiply | 0.069042 | 0.044791 | 1.54x |
| strided / Bf16 / Min | 0.070375 | 0.026917 | 2.61x |
| strided / Bf16 / Max | 0.070417 | 0.027291 | 2.58x |

## Contention and the retained correction

The initial implementation issued packed Min/Max CAS even when an update could
not change its own halfword. Initial hot Max measured about 5-6% slower than the
cast baseline. The retained correction returns when the selected raw value
already equals the observed halfword. For monotonic extrema, that read is a
valid linearization point: further same-target extrema cannot make the update
useful, and a neighboring-halfword write cannot change this decision. Improving
updates still retry whole-word CAS and preserve their neighbor. This also keeps
subnormal and Min=-0 / Max=+0 ordering.

The initial shader, source hashes and full raw run are retained. No benchmark
geometry, input, oracle, baseline or sampling policy changed for the final run.
Both runs are separate measurements, so device state may affect their absolute
timings; the final table compares paths within the same final run.

| Hot case | Initial direct low ms | Final direct low ms | Final cast baseline ms |
| --- | ---: | ---: | ---: |
| F16 Min | 0.039125 | 0.020833 | 0.040833 |
| Bf16 Min | 0.037208 | 0.021167 | 0.041041 |
| F16 Max | 0.050375 | 0.025542 | 0.048833 |
| Bf16 Max | 0.050792 | 0.025750 | 0.048583 |
| F16 Add | 4.815583 | 4.838916 | 4.976500 |
| Bf16 Add | 4.897875 | 4.848167 | 4.962792 |

Hot Add still takes roughly 5 ms in both paths: nearly every colliding addition
changes the value, so avoiding conversion does little to remove CAS contention.
The early return only changes packed Min/Max. Different aggregation would be
needed to address Add; it was not introduced or measured in this phase.

## Memory

Let N be the base/result element count and M the stored update element count.
The cast baseline allocates 4N bytes for expanded base, 4M for expanded updates,
and 4N for the f32 scatter result, in addition to the shared low output.

- Direct Add/Multiply retain the 4N f32 result/accumulator and remove **4N+4M**
  bytes of conversion temporaries.
- Direct Replace/Min/Max work in the low result and remove **8N+4M** bytes.
- Resident low inputs, indices, invalid counts and Replace owner metadata are
  common to both paths. Padding, metadata, staging/readback and native driver
  allocations are separate. These formulas describe visible logical buffers,
  not peak RSS.

For the unique case (N=263425, M=263939), the removed f32 temporaries total
2,109,456 bytes for Add/Multiply and 3,163,156 bytes for Replace/Min/Max.
Broadcast updates can change M; this benchmark uses fully stored updates.

## Evidence and limits

[Final raw run](tensor-low-scatter-metal.txt),
[initial raw run](tensor-low-scatter-initial-metal.txt),
[initial shader](tensor-low-scatter-initial-source/tensor_low_scatter_store.wgsl),
[initial fingerprints](tensor-low-scatter-initial-source-fingerprints.json),
[final fingerprints](tensor-low-scatter-source-fingerprints.json),
[initial focused tests](tensor-low-scatter-metal-tests.txt),
[optimized focused tests](tensor-low-scatter-extrema-metal-tests.txt),
[full qualification](../../../docs/qualification/tensor-low-scatter-2026-09-27.md).

This establishes no CUDA/MLX performance result, cross-device improvement or
application-level throughput gain. Other collision distributions, non-last
scatter axes, broadcast update storage and f32 output performance are unmeasured.
The GPU tests cover those semantics separately. Tiny timing differences may
reflect device state and measurement noise; no significance test was performed.
