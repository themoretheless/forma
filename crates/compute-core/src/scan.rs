use crate::{
    Binding, ComputeBatch, ComputeError, ComputeProgram, ComputeRuntime, GpuArray, Kernel, shaders,
    uniform_f32, wgpu,
};

pub(crate) struct ScanKernels {
    blocks: Kernel,
    add: Kernel,
}

/// Must match ITEMS in scan_blocks4.wgsl; each lane scans four adjacent scalars.
const ITEMS_PER_LANE: u32 = 4;

impl ScanKernels {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, crate::KernelError> {
        Ok(Self {
            blocks: Kernel::new(
                device,
                "exclusive scan blocks",
                shaders::SCAN_BLOCKS4_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                    Binding::StorageReadWrite,
                ],
            )?,
            add: Kernel::new(
                device,
                "exclusive scan offsets",
                shaders::SCAN_ADD_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )?,
        })
    }
}

/// Prepared off to the side so allocation errors never append partial work to
/// the caller's program. Bind groups retain intermediate buffers and uniforms.
pub(crate) struct ScanPlan<'a> {
    steps: Vec<(&'a Kernel, wgpu::BindGroup, u32)>,
}

impl<'a> ScanPlan<'a> {
    pub(crate) fn new(
        runtime: &'a ComputeRuntime,
        input: &GpuArray<u32>,
        output: &GpuArray<u32>,
        normalize: bool,
    ) -> Result<Self, ComputeError> {
        let mut plan = Self {
            steps: Vec::with_capacity(4),
        };
        if !input.is_empty() {
            plan.level(runtime, input, output, normalize)?;
        }
        Ok(plan)
    }

    fn level(
        &mut self,
        runtime: &'a ComputeRuntime,
        input: &GpuArray<u32>,
        output: &GpuArray<u32>,
        normalize: bool,
    ) -> Result<(), ComputeError> {
        let totals = self.record_blocks(runtime, input, output, normalize)?;
        let blocks = totals.len;
        if blocks > 1 {
            let offsets = runtime.zeros::<u32>(blocks as usize)?;
            self.level(runtime, &totals, &offsets, false)?;
            let add = &runtime.scan.add;
            let groups = add.workgroup_count(input.len).min(65535);
            let params = uniform_f32(
                &runtime.device,
                &runtime.queue,
                &[
                    f32::from_bits(input.len),
                    f32::from_bits(groups),
                    f32::from_bits(runtime.scan.blocks.workgroup_size() * ITEMS_PER_LANE),
                    0.0,
                ],
            );
            let bindings = add.create_bind_group(
                &runtime.device,
                &[&params, offsets.buffer(), output.buffer()],
            );
            self.steps.push((add, bindings, groups));
        }
        Ok(())
    }

    fn record_blocks(
        &mut self,
        runtime: &'a ComputeRuntime,
        input: &GpuArray<u32>,
        output: &GpuArray<u32>,
        normalize: bool,
    ) -> Result<GpuArray<u32>, ComputeError> {
        let kernel = &runtime.scan.blocks;
        let blocks = input.len.div_ceil(kernel.workgroup_size() * ITEMS_PER_LANE);
        let groups = blocks.min(65535);
        let totals = runtime.zeros::<u32>(blocks as usize)?;
        let params = uniform_f32(
            &runtime.device,
            &runtime.queue,
            &[
                f32::from_bits(input.len),
                f32::from_bits(groups),
                f32::from_bits(u32::from(normalize)),
                0.0,
            ],
        );
        let bindings = kernel.create_bind_group(
            &runtime.device,
            &[&params, input.buffer(), output.buffer(), totals.buffer()],
        );
        self.steps.push((kernel, bindings, groups));
        Ok(totals)
    }

    /// Produces per-block local offsets and separately scanned block offsets.
    /// Compaction combines them during scatter, avoiding a full-size add pass.
    pub(crate) fn local(
        runtime: &'a ComputeRuntime,
        input: &GpuArray<u32>,
        local: &GpuArray<u32>,
    ) -> Result<(Self, GpuArray<u32>, u32), ComputeError> {
        let mut plan = Self {
            steps: Vec::with_capacity(4),
        };
        let kernel = &runtime.scan.blocks;
        let block_size = kernel.workgroup_size() * ITEMS_PER_LANE;
        let blocks = input.len.div_ceil(block_size);
        let block_offsets = runtime.zeros::<u32>(blocks.max(1) as usize)?;
        if !input.is_empty() {
            let totals = plan.record_blocks(runtime, input, local, true)?;
            if blocks > 1 {
                plan.level(runtime, &totals, &block_offsets, false)?;
            }
        }
        Ok((plan, block_offsets, block_size))
    }

    pub(crate) fn append_to(&self, batch: &mut ComputeBatch<'a>) {
        for (kernel, bindings, groups) in &self.steps {
            batch.push(kernel, bindings, *groups);
        }
    }
}

impl ComputeProgram<'_> {
    /// Exclusive prefix sum: `output[i] = sum(input[..i])`, with u32 wrapping
    /// addition. Empty input produces an empty array. Every level stays on GPU.
    pub fn exclusive_scan(&mut self, input: &GpuArray<u32>) -> Result<GpuArray<u32>, ComputeError> {
        self.runtime.check(input)?;
        let output = self.runtime.zeros(input.len())?;
        self.exclusive_scan_into(input, &output)?;
        Ok(output)
    }

    /// Reuses a distinct, equal-length output allocation. The prepared program
    /// can run repeatedly after input updates without rebuilding scan levels.
    pub fn exclusive_scan_into(
        &mut self,
        input: &GpuArray<u32>,
        output: &GpuArray<u32>,
    ) -> Result<(), ComputeError> {
        self.runtime.check(input)?;
        self.runtime.check(output)?;
        if input.len() != output.len() {
            return Err(ComputeError::LengthMismatch {
                expected: input.len(),
                actual: output.len(),
            });
        }
        if input.aliases(output) {
            return Err(ComputeError::AliasedOutput);
        }
        let plan = ScanPlan::new(self.runtime, input, output, false)?;
        plan.append_to(&mut self.batch);
        Ok(())
    }
}

/// Deterministic CPU reference exclusive prefix scan (`output[i] = sum(input[..i])` with `u32` wrapping addition),
/// matching [`ComputeProgram::exclusive_scan`].
#[inline]
pub fn exclusive_scan_u32(input: &[u32]) -> Vec<u32> {
    let mut acc = 0_u32;
    input
        .iter()
        .map(|&v| {
            let prev = acc;
            acc = acc.wrapping_add(v);
            prev
        })
        .collect()
}

/// Deterministic CPU reference inclusive prefix scan (`output[i] = sum(input[..=i])` with `u32` wrapping addition).
#[inline]
pub fn inclusive_scan_u32(input: &[u32]) -> Vec<u32> {
    let mut acc = 0_u32;
    input
        .iter()
        .map(|&v| {
            acc = acc.wrapping_add(v);
            acc
        })
        .collect()
}

/// Deterministic CPU reference stream compaction keeping elements where `keep[i] != 0` in input order.
pub fn compact_if_u32<T: Copy>(input: &[T], keep: &[u32]) -> Result<Vec<T>, ComputeError> {
    if input.len() != keep.len() {
        return Err(ComputeError::LengthMismatch {
            expected: input.len(),
            actual: keep.len(),
        });
    }
    let count = keep.iter().filter(|&&m| m != 0).count();
    let mut out = Vec::with_capacity(count);
    for (&val, &mask) in input.iter().zip(keep.iter()) {
        if mask != 0 {
            out.push(val);
        }
    }
    Ok(out)
}

/// 4-pass 8-bit Least-Significant-Digit (LSD) radix sort over `u32` keys using stack-allocated
/// `[usize; 256]` counting/prefix-scan histograms ($O(N)$ linear time, zero comparison branching).
///
/// Designed for high-throughput BVH Morton code and spatial particle sorting.
pub fn radix_sort_u32(values: &[u32]) -> Vec<u32> {
    let n = values.len();
    if n <= 1 {
        return values.to_vec();
    }
    let mut src = values.to_vec();
    let mut dst = vec![0_u32; n];

    for pass in 0..4 {
        let shift = pass * 8;
        let mut counts = [0_usize; 256];
        for &v in &src {
            let bucket = ((v >> shift) & 0xFF) as usize;
            counts[bucket] += 1;
        }
        // If all elements fall into a single bucket in this byte, skip the scatter pass
        if counts.contains(&n) {
            continue;
        }
        let mut offsets = [0_usize; 256];
        let mut sum = 0_usize;
        for b in 0..256 {
            offsets[b] = sum;
            sum += counts[b];
        }
        for &v in &src {
            let bucket = ((v >> shift) & 0xFF) as usize;
            let pos = offsets[bucket];
            dst[pos] = v;
            offsets[bucket] = pos + 1;
        }
        std::mem::swap(&mut src, &mut dst);
    }
    src
}

/// Stable 4-pass 8-bit LSD radix sort of `(keys, payloads)` pairs by `u32` key in $O(N)$ linear time.
///
/// Preserves relative order of elements with equal keys. If `keys.len() != payloads.len()`, sorts
/// the prefix of length `keys.len().min(payloads.len())`.
pub fn radix_sort_by_key_u32<T: Copy>(keys: &[u32], payloads: &[T]) -> (Vec<u32>, Vec<T>) {
    let n = keys.len().min(payloads.len());
    if n <= 1 {
        return (keys[..n].to_vec(), payloads[..n].to_vec());
    }
    let mut src_keys = keys[..n].to_vec();
    let mut dst_keys = vec![0_u32; n];
    let mut src_vals = payloads[..n].to_vec();
    let mut dst_vals = payloads[..n].to_vec();

    for pass in 0..4 {
        let shift = pass * 8;
        let mut counts = [0_usize; 256];
        for &k in &src_keys {
            let bucket = ((k >> shift) & 0xFF) as usize;
            counts[bucket] += 1;
        }
        if counts.contains(&n) {
            continue;
        }
        let mut offsets = [0_usize; 256];
        let mut sum = 0_usize;
        for b in 0..256 {
            offsets[b] = sum;
            sum += counts[b];
        }
        for (&k, &val) in src_keys.iter().zip(src_vals.iter()) {
            let bucket = ((k >> shift) & 0xFF) as usize;
            let pos = offsets[bucket];
            dst_keys[pos] = k;
            dst_vals[pos] = val;
            offsets[bucket] = pos + 1;
        }
        std::mem::swap(&mut src_keys, &mut dst_keys);
        std::mem::swap(&mut src_vals, &mut dst_vals);
    }
    (src_keys, src_vals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_reference_scans_and_compaction_match_expected_semantics() {
        let data = [3_u32, 1, 4, 1, 5, 9];
        assert_eq!(exclusive_scan_u32(&data), vec![0, 3, 4, 8, 9, 14]);
        assert_eq!(inclusive_scan_u32(&data), vec![3, 4, 8, 9, 14, 23]);
        assert!(exclusive_scan_u32(&[]).is_empty());
        assert!(inclusive_scan_u32(&[]).is_empty());

        let values = [10.0_f32, 20.0, 30.0, 40.0, 50.0];
        let keep = [0_u32, 1, 0, 5, 1];
        let compacted = compact_if_u32(&values, &keep).unwrap();
        assert_eq!(compacted, vec![20.0, 40.0, 50.0]);
        assert!(compact_if_u32(&values, &[1, 0]).is_err());

        let unsorted = [0xFFFF_FFFF_u32, 42, 0, 1024, 7, 42, 256, 65535];
        let sorted = radix_sort_u32(&unsorted);
        assert_eq!(sorted, vec![0, 7, 42, 42, 256, 1024, 65535, 0xFFFF_FFFF]);

        let keys = [300_u32, 10, 200, 10, 5];
        let payloads = [0_usize, 1, 2, 3, 4];
        let (sk, sp) = radix_sort_by_key_u32(&keys, &payloads);
        assert_eq!(sk, vec![5, 10, 10, 200, 300]);
        // Stability check: key 10 at index 1 comes before key 10 at index 3
        assert_eq!(sp, vec![4, 1, 3, 2, 0]);
    }
}
