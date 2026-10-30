# Native Metal shared memory

Enable `native-wgpu` to use `metal_memory::MetalSharedBuffer` on macOS.
This optional bridge targets wgpu 30; the existing gpu-runtime API is unchanged.
The buffer owns page-aligned host memory and its Metal/wgpu handles, with shared
lifetime across clones. It contains no media formats, codecs or rendering policy.

CPU reads/writes require the caller to synchronize GPU access: complete relevant
queue submissions before host reads or writes. Host copies enforce logical byte
length; Metal allocation capacity is rounded to 16 KiB pages. Buffer usage and
queue operations must satisfy wgpu alignment and usage rules.

Extracted from fvid's generic UMA allocation; source notices are retained in
LICENSE-fvid and LICENSE-MPL-2.0-fvid. fvid's CoreVideo/YUV/rendering/CUDA code
remains in fvid. Physical GPU tests are opt-in and do not accept CPU fallback.
