//! Deterministic zero-dependency CPU data-parallel primitives.
//!
//! On native targets (`not(target_arch = "wasm32")`), work is partitioned into
//! contiguous slices and dispatched across `std::thread::scope` using up to
//! `std::thread::available_parallelism()` worker threads when the total work
//! exceeds `min_chunk`. On `wasm32` (or when input size is small / single-core),
//! execution falls back to a direct sequential loop with zero overhead.

/// Returns the number of hardware threads available for scoped parallel execution.
#[inline]
pub fn available_threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// Maps a slice `items` in parallel into an ordered `Vec<R>` when `items.len() > min_chunk`
/// and multiple hardware threads are available. Preserves exact element order.
pub fn par_map<T, R, F>(items: &[T], min_chunk: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let len = items.len();
    if len == 0 {
        return Vec::new();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let min_chunk = min_chunk.max(1);
        let threads = available_threads();
        if threads > 1 && len > min_chunk {
            let chunk_size = len.div_ceil(threads).max(min_chunk);
            if len > chunk_size {
                let mut out = Vec::with_capacity(len);
                std::thread::scope(|s| {
                    let mut handles = Vec::with_capacity(len.div_ceil(chunk_size));
                    for chunk in items.chunks(chunk_size) {
                        let f_ref = &f;
                        handles.push(s.spawn(move || {
                            let mut part = Vec::with_capacity(chunk.len());
                            for item in chunk {
                                part.push(f_ref(item));
                            }
                            part
                        }));
                    }
                    for handle in handles {
                        out.extend(handle.join().expect("parallel worker thread panicked"));
                    }
                });
                return out;
            }
        }
    }
    let _ = min_chunk;
    items.iter().map(f).collect()
}

/// Maps index range `0..len` in parallel into an ordered `Vec<R>` when `len > min_chunk`
/// and multiple hardware threads are available. Preserves exact index order `0..len`.
pub fn par_map_index<R, F>(len: usize, min_chunk: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(usize) -> R + Sync,
{
    if len == 0 {
        return Vec::new();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let min_chunk = min_chunk.max(1);
        let threads = available_threads();
        if threads > 1 && len > min_chunk {
            let chunk_size = len.div_ceil(threads).max(min_chunk);
            if len > chunk_size {
                let mut out = Vec::with_capacity(len);
                std::thread::scope(|s| {
                    let mut handles = Vec::with_capacity(len.div_ceil(chunk_size));
                    let mut start = 0;
                    while start < len {
                        let end = (start + chunk_size).min(len);
                        let f_ref = &f;
                        handles.push(s.spawn(move || {
                            let mut part = Vec::with_capacity(end - start);
                            for idx in start..end {
                                part.push(f_ref(idx));
                            }
                            part
                        }));
                        start = end;
                    }
                    for handle in handles {
                        out.extend(handle.join().expect("parallel worker thread panicked"));
                    }
                });
                return out;
            }
        }
    }
    let _ = min_chunk;
    (0..len).map(f).collect()
}

/// Fallible parallel map over `items`, preserving deterministic first-error semantics.
pub fn par_try_map<T, R, E, F>(items: &[T], min_chunk: usize, f: F) -> std::result::Result<Vec<R>, E>
where
    T: Sync,
    R: Send,
    E: Send,
    F: Fn(&T) -> std::result::Result<R, E> + Sync,
{
    let len = items.len();
    if len == 0 {
        return Ok(Vec::new());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let min_chunk = min_chunk.max(1);
        let threads = available_threads();
        if threads > 1 && len > min_chunk {
            let chunk_size = len.div_ceil(threads).max(min_chunk);
            if len > chunk_size {
                let mut out = Vec::with_capacity(len);
                let mut first_err: Option<E> = None;
                std::thread::scope(|s| {
                    let mut handles = Vec::with_capacity(len.div_ceil(chunk_size));
                    for chunk in items.chunks(chunk_size) {
                        let f_ref = &f;
                        handles.push(s.spawn(move || {
                            let mut part = Vec::with_capacity(chunk.len());
                            for item in chunk {
                                part.push(f_ref(item)?);
                            }
                            Ok::<Vec<R>, E>(part)
                        }));
                    }
                    for handle in handles {
                        match handle.join().expect("parallel worker thread panicked") {
                            Ok(part) if first_err.is_none() => out.extend(part),
                            Ok(_) => {}
                            Err(err) if first_err.is_none() => first_err = Some(err),
                            Err(_) => {}
                        }
                    }
                });
                return match first_err {
                    Some(err) => Err(err),
                    None => Ok(out),
                };
            }
        }
    }
    let _ = min_chunk;
    items.iter().map(f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par_map_preserves_exact_order() {
        let input: Vec<usize> = (0..500).collect();
        let out = par_map(&input, 16, |&x| x * x + 3);
        let expected: Vec<usize> = input.iter().map(|&x| x * x + 3).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn par_map_index_preserves_exact_order() {
        let out = par_map_index(300, 16, |i| i * 7 + 1);
        let expected: Vec<usize> = (0..300).map(|i| i * 7 + 1).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn par_try_map_returns_earliest_chunk_error() {
        let input: Vec<usize> = (0..128).collect();
        let res: std::result::Result<Vec<usize>, usize> = par_try_map(&input, 8, |&x| {
            if x == 19 || x == 95 {
                Err(x)
            } else {
                Ok(x * 2)
            }
        });
        assert_eq!(res, Err(19));
    }
}
