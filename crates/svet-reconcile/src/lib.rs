//! Zero-allocation $O(n \log n)$ Longest Increasing Subsequence (LIS) keyed-list
//! reconciler. Separated from `svet-graph` and `svet-gpu-arena` so list diffing
//! scratch buffers live independently of reactive DAG and GPU state.

/// Reusable scratch storage for $O(n \log n)$ LIS computation.
#[derive(Debug, Default)]
pub struct LisScratch {
    tails: Vec<usize>,
    predecessors: Vec<usize>,
}

impl LisScratch {
    pub const fn new() -> Self {
        Self {
            tails: Vec::new(),
            predecessors: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.tails.clear();
        self.predecessors.clear();
    }

    /// Computes the Longest Increasing Subsequence of `input`, ignoring `u32::MAX`
    /// markers (which represent newly inserted items).
    ///
    /// Writes ascending input indices into `out` and returns the LIS length.
    /// Runs in $O(n \log n)$ time with zero heap allocations once warmed.
    pub fn compute_into(&mut self, input: &[u32], out: &mut Vec<u32>) -> usize {
        let n = input.len();
        out.clear();
        if n == 0 {
            return 0;
        }

        self.tails.clear();
        if self.predecessors.len() < n {
            self.predecessors.resize(n, usize::MAX);
        }

        for (i, &val) in input.iter().enumerate() {
            if val == u32::MAX {
                continue;
            }

            if self.tails.last().is_none_or(|&last| input[last] < val) {
                self.predecessors[i] = self.tails.last().copied().unwrap_or(usize::MAX);
                self.tails.push(i);
            } else {
                let low = self.tails.partition_point(|&idx| input[idx] < val);
                if val < input[self.tails[low]] {
                    self.predecessors[i] = if low > 0 {
                        self.tails[low - 1]
                    } else {
                        usize::MAX
                    };
                    self.tails[low] = i;
                }
            }
        }

        let lis_len = self.tails.len();
        if lis_len == 0 {
            return 0;
        }

        out.resize(lis_len, 0);
        let mut k = self.tails[lis_len - 1];
        for slot in out.iter_mut().rev() {
            *slot = k as u32;
            k = self.predecessors[k];
        }

        lis_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_minimal_moves_and_ignores_new_item_markers() {
        let mut lis = LisScratch::new();
        let mut out = Vec::new();
        let input = [2, 1, u32::MAX, 5, 3, 6, 4, u32::MAX, 8, 9, 7];
        let len = lis.compute_into(&input, &mut out);
        assert_eq!(len, 5);
        let values: Vec<u32> = out.iter().map(|&idx| input[idx as usize]).collect();
        assert_eq!(values, vec![1, 3, 4, 8, 9]);
    }

    #[test]
    fn handles_empty_and_all_new_inputs_and_reuses_warmed_buffers() {
        let mut lis = LisScratch::new();
        let mut out = Vec::new();
        assert_eq!(lis.compute_into(&[], &mut out), 0);
        assert!(out.is_empty());
        assert_eq!(lis.compute_into(&[u32::MAX, u32::MAX], &mut out), 0);
        assert!(out.is_empty());

        let sample = [10, 20, 5, 30, 40];
        assert_eq!(lis.compute_into(&sample, &mut out), 4);
        let tails_ptr = lis.tails.as_ptr();
        let preds_ptr = lis.predecessors.as_ptr();
        let out_ptr = out.as_ptr();
        for _ in 0..64 {
            assert_eq!(lis.compute_into(&sample, &mut out), 4);
        }
        assert_eq!(lis.tails.as_ptr(), tails_ptr);
        assert_eq!(lis.predecessors.as_ptr(), preds_ptr);
        assert_eq!(out.as_ptr(), out_ptr);
    }
}
