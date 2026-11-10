use crate::graph::ReactiveGraph;

impl ReactiveGraph {
    /// Computes Longest Increasing Subsequence (LIS) of `input` (ignoring `u32::MAX` which denotes new items).
    /// Writes indices of the LIS in ascending order into `self.lis_buffer` and returns the count.
    /// Complexity: O(n log n) time, 0 per-call heap allocations once warmed.
    pub fn compute_lis(&mut self, input: &[u32]) -> usize {
        let n = input.len();
        self.lis_buffer.clear();
        if n == 0 {
            return 0;
        }

        self.lis_tails.clear();
        if self.lis_predecessors.len() < n {
            self.lis_predecessors.resize(n, usize::MAX);
        }

        for i in 0..n {
            let val = input[i];
            if val == u32::MAX {
                continue;
            }

            if self.lis_tails.is_empty() || input[*self.lis_tails.last().unwrap()] < val {
                self.lis_predecessors[i] = self.lis_tails.last().copied().unwrap_or(usize::MAX);
                self.lis_tails.push(i);
            } else {
                let mut low = 0usize;
                let mut high = self.lis_tails.len() - 1;
                while low < high {
                    let mid = low + (high - low) / 2;
                    if input[self.lis_tails[mid]] < val {
                        low = mid + 1;
                    } else {
                        high = mid;
                    }
                }
                if val < input[self.lis_tails[low]] {
                    self.lis_predecessors[i] = if low > 0 {
                        self.lis_tails[low - 1]
                    } else {
                        usize::MAX
                    };
                    self.lis_tails[low] = i;
                }
            }
        }

        let lis_len = self.lis_tails.len();
        if lis_len == 0 {
            return 0;
        }

        self.lis_buffer.resize(lis_len, 0);
        let mut k = self.lis_tails[lis_len - 1];
        for idx in (0..lis_len).rev() {
            self.lis_buffer[idx] = k as u32;
            k = self.lis_predecessors[k];
        }

        lis_len
    }
}
