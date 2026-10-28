/// Compute placement. `Cpu` is the deterministic reference; `Auto` lets the
/// callee choose from the problem size; the device placements are opt-in and
/// fall back to the CPU reference when unavailable.
///
/// - `Auto`: size-based placement selection, tuned per kernel.
/// - `Gpu`: portable compute shaders (wgpu — Vulkan/DX12/Metal natively,
///   WebGPU in the browser). Reaches NVIDIA hardware through Vulkan/DX12.
/// - `Cuda`: the CUDA driver API on NVIDIA hardware (native `cuda` feature).
///   Kernels without a CUDA port run their `Gpu` shader instead, so `Cuda`
///   is never slower than `Gpu` on the same device; without a CUDA device
///   the `Gpu` path is tried, then the CPU reference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Acceleration {
    #[default]
    Cpu,
    Auto,
    Gpu,
    Cuda,
}

impl Acceleration {
    /// True for every device-capable placement (`Auto`, `Gpu`, or `Cuda`);
    /// kernels that only have a portable shader use this instead of comparing
    /// against `Gpu`. Kernels with a real size heuristic should resolve
    /// `Auto` first, as [`crate::nearest_neighbor_accelerated`] does.
    #[inline]
    pub const fn is_gpu(self) -> bool {
        matches!(self, Self::Auto | Self::Gpu | Self::Cuda)
    }

    /// Parses the CLI/environment spelling (`cpu`, `auto`, `gpu`, `metal`,
    /// `cuda`). `metal` maps to `Gpu`, the portable wgpu backend that binds
    /// Metal on macOS and Vulkan/DX12 elsewhere.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(Self::Cpu),
            "auto" => Some(Self::Auto),
            "gpu" | "wgpu" | "webgpu" | "metal" => Some(Self::Gpu),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Cuda => "cuda",
        }
    }

    /// Suggests a placement for [`crate::nearest_neighbor_accelerated`] from the
    /// problem size alone, so callers don't have to hand-tune a threshold or
    /// benchmark their own workload before picking an `Acceleration`.
    ///
    /// Based on `queries.len() * targets.len()` ("work"): brute-force
    /// nearest-neighbor is O(work), so it — not either dimension alone —
    /// governs the crossover. Measured with `examples/bench_gpu.rs` on an
    /// NVIDIA RTX 5090 (CUDA 13.4): the CPU reference wins up to work ≈
    /// 100K (e.g. 1,000×100, cpu 0.078ms vs cuda 0.137ms) but CUDA already
    /// wins by 300K (3,000×100, cpu 0.237ms vs cuda 0.138ms) and wins by
    /// 15-300x from 10M work upward; wgpu crosses over later, roughly
    /// breaking even around 300K-1M work before pulling ahead. This is a
    /// starting point tuned to that hardware, not a guarantee for every
    /// GPU/CUDA device — re-benchmark for workloads where the choice
    /// matters.
    pub fn recommended_for_nearest_neighbor(query_count: usize, target_count: usize) -> Self {
        const CUDA_WORK_THRESHOLD: usize = 200_000;
        const GPU_WORK_THRESHOLD: usize = 700_000;
        let Some(work) = query_count.checked_mul(target_count) else {
            return if crate::device::cuda_available() {
                Self::Cuda
            } else {
                Self::Gpu
            };
        };
        if crate::device::cuda_available() && work >= CUDA_WORK_THRESHOLD {
            Self::Cuda
        } else if work >= GPU_WORK_THRESHOLD {
            Self::Gpu
        } else {
            Self::Cpu
        }
    }

    /// Resolves `Auto` for [`crate::nearest_neighbor_accelerated`]; explicit
    /// placements pass through unchanged.
    pub fn resolve_for_nearest_neighbor(self, query_count: usize, target_count: usize) -> Self {
        match self {
            Self::Auto => Self::recommended_for_nearest_neighbor(query_count, target_count),
            explicit => explicit,
        }
    }

    /// Suggests a placement for one-to-one batched squared distances. This is
    /// O(pair_count) with only a few FLOPs per element; measured GPU/CUDA
    /// paths do not amortize launch/copy/readback overhead, so Auto preserves
    /// the CPU reference. Explicit `Gpu`/`Cuda` still force device execution.
    pub const fn recommended_for_distance_pairs(pair_count: usize) -> Self {
        let _ = pair_count;
        Self::Cpu
    }

    /// Resolves `Auto` for batched one-to-one squared distances; explicit
    /// placements pass through unchanged.
    pub const fn resolve_for_distance_pairs(self, pair_count: usize) -> Self {
        match self {
            Self::Auto => Self::recommended_for_distance_pairs(pair_count),
            explicit => explicit,
        }
    }

    /// Suggests a placement for fused point-cloud summary reductions
    /// (bounds + moments). wgpu is upload-bound on the measured discrete GPU,
    /// but the fused CUDA path beats the CPU reference for large clouds.
    pub fn recommended_for_point_cloud_stats(point_count: usize) -> Self {
        const CUDA_POINT_THRESHOLD: usize = 500_000;
        if crate::device::cuda_available() && point_count >= CUDA_POINT_THRESHOLD {
            Self::Cuda
        } else {
            Self::Cpu
        }
    }

    /// Resolves `Auto` for fused point-cloud summary reductions.
    pub fn resolve_for_point_cloud_stats(self, point_count: usize) -> Self {
        match self {
            Self::Auto => Self::recommended_for_point_cloud_stats(point_count),
            explicit => explicit,
        }
    }

    /// Suggests a placement for transformed point-cloud bounds. This fused
    /// O(n) kernel is close enough to the CPU reference that measured device
    /// wins were not stable, so Auto preserves the CPU path. Explicit
    /// `Gpu`/`Cuda` still force device execution.
    pub const fn recommended_for_transformed_point_bounds(point_count: usize) -> Self {
        let _ = point_count;
        Self::Cpu
    }

    /// Resolves `Auto` for transformed point-cloud bounds.
    pub const fn resolve_for_transformed_point_bounds(self, point_count: usize) -> Self {
        match self {
            Self::Auto => Self::recommended_for_transformed_point_bounds(point_count),
            explicit => explicit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceleration_parse_and_placement() {
        assert_eq!(Acceleration::parse("cpu"), Some(Acceleration::Cpu));
        assert_eq!(Acceleration::parse("auto"), Some(Acceleration::Auto));
        assert_eq!(Acceleration::parse(" GPU "), Some(Acceleration::Gpu));
        assert_eq!(Acceleration::parse("metal"), Some(Acceleration::Gpu));
        assert_eq!(Acceleration::parse("webgpu"), Some(Acceleration::Gpu));
        assert_eq!(Acceleration::parse("cuda"), Some(Acceleration::Cuda));
        assert_eq!(Acceleration::parse("opencl"), None);
        assert!(!Acceleration::Cpu.is_gpu());
        assert!(Acceleration::Auto.is_gpu());
        assert!(Acceleration::Gpu.is_gpu());
        assert!(Acceleration::Cuda.is_gpu());
        for mode in [
            Acceleration::Cpu,
            Acceleration::Auto,
            Acceleration::Gpu,
            Acceleration::Cuda,
        ] {
            assert_eq!(Acceleration::parse(mode.label()), Some(mode));
        }
        assert_eq!(Acceleration::default(), Acceleration::Cpu);
    }

    #[test]
    fn recommended_for_nearest_neighbor_scales_with_work() {
        assert_eq!(
            Acceleration::recommended_for_nearest_neighbor(1_000, 100),
            Acceleration::Cpu
        );
        let huge = Acceleration::recommended_for_nearest_neighbor(1_000_000, 2_000);
        assert!(huge.is_gpu());
        assert!(Acceleration::recommended_for_nearest_neighbor(usize::MAX, 2).is_gpu());
        assert_eq!(
            Acceleration::recommended_for_nearest_neighbor(0, 0),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Cpu.resolve_for_nearest_neighbor(1_000_000, 2_000),
            Acceleration::Cpu
        );
        assert!(
            Acceleration::Auto
                .resolve_for_nearest_neighbor(1_000_000, 2_000)
                .is_gpu()
        );
    }

    #[test]
    fn recommended_for_distance_pairs_keeps_small_batches_on_cpu() {
        assert_eq!(
            Acceleration::recommended_for_distance_pairs(10_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::recommended_for_distance_pairs(usize::MAX),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Cpu.resolve_for_distance_pairs(2_000_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Auto.resolve_for_distance_pairs(2_000_000),
            Acceleration::Cpu
        );
    }

    #[test]
    fn recommended_for_point_cloud_stats_uses_cuda_only_for_large_clouds() {
        assert_eq!(
            Acceleration::recommended_for_point_cloud_stats(10_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Auto.resolve_for_point_cloud_stats(10_000),
            Acceleration::Cpu
        );
        let large = Acceleration::recommended_for_point_cloud_stats(1_000_000);
        if crate::device::cuda_available() {
            assert_eq!(large, Acceleration::Cuda);
        } else {
            assert_eq!(large, Acceleration::Cpu);
        }
        assert_eq!(
            Acceleration::Gpu.resolve_for_point_cloud_stats(10_000),
            Acceleration::Gpu
        );
    }

    #[test]
    fn recommended_for_transformed_point_bounds_stays_on_cpu() {
        assert_eq!(
            Acceleration::recommended_for_transformed_point_bounds(10_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Auto.resolve_for_transformed_point_bounds(10_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::recommended_for_transformed_point_bounds(1_000_000),
            Acceleration::Cpu
        );
        assert_eq!(
            Acceleration::Gpu.resolve_for_transformed_point_bounds(10_000),
            Acceleration::Gpu
        );
    }
}
