# Packed low arithmetic and reductions on Metal

Date: 2026-09-27. Device: Apple M4 Max, 40 GPU cores, macOS 26.7.
This measures the new direct packed f16/BF16 kernels against a resident public
API composition that expands the input to f32. It does not compare native
half instructions, Tensor Cores or MLX/CUDA performance.

## Method

[Runnable benchmark](../examples/bench_tensor_low_ops.rs),
[complete raw samples](tensor-low-ops-metal.txt),
[source fingerprints](tensor-low-ops-source-fingerprints.json).

- Both paths start from the same resident low input and produce the same dtype
  and shape. Uploads, allocation, shader compilation and program recording are
  outside timing; each prepared program is reused.
- Square: baseline low -> f32 -> square -> low, versus a direct packed square
  with one final low rounding. Both return packed low storage.
- Sum: baseline low -> f32 -> sum, versus decode-on-load reduction. Both return
  f32. The all-input baseline retains the existing specialized f32 sum path.
- 200 ms warmup per case, 31 samples, rotating path order and alternating
  profiled/unprofiled order. GPU timestamps enclose the recorded compute pass.
  Separate host timing includes encoder construction, submission, waiting and
  readback; CPU result validation follows the timer.
- Every execution checks every result exactly. Independent fixed low encodings
  represent quarter-valued inputs and their squares; a CPU f64 reference computes
  sums. No GPU conversion supplies the expected values.
- Development build, idle task GPU window. Other app/OS activity is uncontrolled.
  These are local microbenchmarks without a cross-device performance guarantee.

Shapes: small=17x19, rows=1024x1025, all=1x1048581,
strided=257x4097 with a physically transposed input. Sum contracts axis 1;
the strided comparison charges the baseline for materialization while the
direct path traverses the original view.

## Median results, milliseconds

| Case | Baseline GPU | Direct GPU | GPU speedup | Baseline host+readback | Direct host+readback |
| --- | ---: | ---: | ---: | ---: | ---: |
| small_F16_square | 0.011167 | 0.006833 | 1.63x | 0.271959 | 0.225334 |
| small_F16_sum | 0.008375 | 0.007833 | 1.07x | 0.249125 | 0.228042 |
| small_Bf16_square | 0.010833 | 0.006709 | 1.61x | 0.270500 | 0.220959 |
| small_Bf16_sum | 0.008458 | 0.007333 | 1.15x | 0.248167 | 0.222916 |
| rows_F16_square | 0.542125 | 0.212041 | 2.56x | 4.425375 | 3.976750 |
| rows_F16_sum | 0.088792 | 0.043208 | 2.05x | 0.343625 | 0.270125 |
| rows_Bf16_square | 0.523625 | 0.179916 | 2.91x | 4.389750 | 3.964209 |
| rows_Bf16_sum | 0.087916 | 0.039000 | 2.25x | 0.335417 | 0.262333 |
| all_F16_square | 0.540833 | 0.211750 | 2.55x | 4.432917 | 4.071875 |
| all_F16_sum | 0.067125 | 0.040417 | 1.66x | 0.334584 | 0.283459 |
| all_Bf16_square | 0.521292 | 0.178917 | 2.91x | 4.520833 | 4.110250 |
| all_Bf16_sum | 0.067125 | 0.038416 | 1.75x | 0.330375 | 0.277083 |
| strided_F16_square | 0.556250 | 0.214125 | 2.60x | 4.499542 | 4.092583 |
| strided_F16_sum | 0.099875 | 0.046708 | 2.14x | 0.376042 | 0.297792 |
| strided_Bf16_square | 0.549583 | 0.181959 | 3.02x | 4.443083 | 4.005458 |
| strided_Bf16_sum | 0.101125 | 0.044458 | 2.27x | 0.388875 | 0.314333 |

Square improved 1.61-3.02x and sum 1.07-2.28x in these 16 cases. The tiny sum
improvement is only about 0.5-1.1 microseconds, so its magnitude is particularly
sensitive to hardware and sampling conditions. Large square readbacks dominate
host time, making the end-to-end difference much smaller than GPU speedup.
No timing conclusion is established for the other unary/binary operations,
product/extrema/mean, different distributions, browsers or other backends.

## Allocation accounting

Both paths keep the same packed input and final output. The baseline additionally
holds 4*N bytes for expanded input; square adds another 4*N bytes for its f32
intermediate. Direct square has no tensor-sized intermediate. At N=1,048,581,
this removes 8,388,648 bytes of temporary arrays for square and 4,194,324 bytes
of input expansion for sum. The packed input itself occupies 2,097,164 bytes,
including the final halfword padding.

Direct sum may allocate f32 partials. The current planner bounds them to
4096 values (16 KiB) in total, at most 256 parts per output. The all-input case
uses 256 partial values (1024 bytes). This accounting excludes metadata,
readback buffers, driver allocations and registers/spills; it is not peak RSS
or a measured device memory trace.

See [numerical/recording contracts](tensor-low-ops-contracts.md) and the
[complete qualification](../../../docs/qualification/tensor-low-ops-2026-09-27.md).
