//! A single swap object with managed per-block integrity for bounded random reads.
use super::{BLOCK, Result, SwapError, SwapHandle, SwapStore};
use memory_budget::{MemoryBudget, SharedBuffer};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    sync::Arc,
};

#[derive(Debug)]
struct Index {
    blob: SwapHandle,
    digests: SharedBuffer<[u8; 32]>,
}

/// Immutable ordered blocks in one disk object. Clones share disk and index credit.
/// Integrity is checked for requested blocks, not for unread blocks.
#[derive(Debug, Clone)]
pub struct SegmentedSwapHandle(Arc<Index>);
impl SegmentedSwapHandle {
    pub fn byte_len(&self) -> u64 {
        self.0.blob.byte_len()
    }
    pub fn block_count(&self) -> usize {
        self.0.digests.len()
    }
}

/// Byte layout of non-overlapping source rows; output contains only row bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapRows {
    pub offset: u64,
    pub row_bytes: u64,
    pub stride: u64,
    pub rows: u64,
}

/// Scoped reader for an immutable segmented object. Retains one verified block;
/// cached bytes remain a verified snapshot even if the backing file is damaged later.
#[derive(Debug)]
pub struct SegmentedSwapReader {
    handle: SegmentedSwapHandle,
    file: std::fs::File,
    scratch: memory_budget::MutableBuffer<u8>,
    cached_index: Option<usize>,
    next_file_offset: Option<u64>,
}
impl SegmentedSwapReader {
    /// Copy a verified range into caller-owned storage. On failure the destination
    /// may be partially written; callers must not publish it until success.
    ///
    /// # Errors
    /// Rejects invalid ranges, cancellation, corrupt blocks and file I/O failures.
    pub fn read_into(
        &mut self,
        offset: u64,
        output: &mut [u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<()> {
        let end = offset
            .checked_add(output.len() as u64)
            .filter(|end| *end <= self.handle.byte_len())
            .ok_or(SwapError::Length)?;
        let mut position = offset;
        let mut target = 0;
        while position < end {
            if cancelled() {
                return Err(SwapError::Cancelled);
            }
            let index = usize::try_from(position / BLOCK as u64).map_err(|_| SwapError::Length)?;
            let block_offset = position / BLOCK as u64 * BLOCK as u64;
            let length = usize::try_from((self.handle.byte_len() - block_offset).min(BLOCK as u64))
                .map_err(|_| SwapError::Length)?;
            if self.cached_index != Some(index) {
                // Invalidate before overwriting: a failed read must never leave a
                // formerly verified index attached to partially replaced bytes.
                self.cached_index = None;
                if self.next_file_offset != Some(block_offset) {
                    self.next_file_offset = None;
                    self.file.seek(SeekFrom::Start(block_offset))?;
                }
                // A partial failed read leaves the cursor unknown; retry must seek.
                self.next_file_offset = None;
                self.file.read_exact(&mut self.scratch[..length])?;
                self.next_file_offset = Some(block_offset + length as u64);
                if cancelled() {
                    return Err(SwapError::Cancelled);
                }
                if blake3::hash(&self.scratch[..length]).as_bytes() != &self.handle.0.digests[index]
                {
                    return Err(SwapError::Corrupt);
                }
                self.cached_index = Some(index);
            }
            let start = usize::try_from(position - block_offset).map_err(|_| SwapError::Length)?;
            let count = usize::try_from((end - position).min((length - start) as u64))
                .map_err(|_| SwapError::Length)?;
            output[target..target + count].copy_from_slice(&self.scratch[start..start + count]);
            target += count;
            position += count as u64;
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        Ok(())
    }
}

impl SwapStore {
    /// Open once and retain a bounded verified block across range reads.
    ///
    /// # Errors
    /// Rejects foreign handles, cancellation, insufficient scratch credit and invalid files.
    pub fn segmented_reader(
        &self,
        handle: &SegmentedSwapHandle,
        scratch_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SegmentedSwapReader> {
        if !Arc::ptr_eq(&self.0, &handle.0.blob.0.reservation.state) {
            return Err(SwapError::ForeignHandle);
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let scratch_len =
            usize::try_from(handle.byte_len().min(BLOCK as u64)).map_err(|_| SwapError::Length)?;
        let scratch = scratch_budget.try_buffer(scratch_len, 0_u8)?;
        let file = handle.0.blob.0.file.reopen()?;
        if file.metadata()?.len() != handle.byte_len() {
            return Err(SwapError::Length);
        }
        Ok(SegmentedSwapReader {
            handle: handle.clone(),
            file,
            scratch,
            cached_index: None,
            next_file_offset: None,
        })
    }

    /// Encode directly into one indexed swap object. Only the digest index is
    /// retained in managed RAM; no complete serialized staging copy is required.
    ///
    /// # Errors
    /// Rejects index admission, disk quota, cancellation, length and encoder/I/O failures.
    pub fn write_segmented_stream(
        &self,
        bytes: u64,
        index_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
        encode: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<SegmentedSwapHandle> {
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let count = usize::try_from(bytes.div_ceil(BLOCK as u64)).map_err(|_| SwapError::Length)?;
        let mut digests = index_budget.try_buffer(count, [0_u8; 32])?;
        let blob = self.write_stream(bytes, &mut cancelled, |writer| {
            let mut indexed = IndexedWriter {
                writer,
                digests: &mut digests,
                hasher: blake3::Hasher::new(),
                used: 0,
                index: 0,
            };
            encode(&mut indexed)?;
            if indexed.used > 0 {
                let digest = indexed
                    .digests
                    .get_mut(indexed.index)
                    .ok_or_else(|| std::io::Error::other(SwapError::Length))?;
                *digest = *indexed.hasher.finalize().as_bytes();
            }
            indexed.flush()
        })?;
        Ok(SegmentedSwapHandle(Arc::new(Index {
            blob,
            digests: digests.freeze(),
        })))
    }
    /// Stream into one disk object with independently verified64KiB blocks.
    /// Index and one scratch block are admitted before allocation. Failure rolls
    /// back the entire object quota; no complete serialized blob is kept in RAM.
    /// Entry/Arc bookkeeping follows the existing store metadata contract.
    ///
    /// # Errors
    /// Rejects memory/disk admission, cancellation, source length and I/O failures.
    pub fn write_segmented(
        &self,
        reader: &mut dyn Read,
        bytes: u64,
        index_budget: &MemoryBudget,
        scratch_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SegmentedSwapHandle> {
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let count = usize::try_from(bytes.div_ceil(BLOCK as u64)).map_err(|_| SwapError::Length)?;
        let usage = self.usage()?;
        if bytes > self.0.limits.max_bytes.saturating_sub(usage.bytes)
            || self
                .0
                .limits
                .max_objects
                .is_some_and(|max| usage.objects >= max)
        {
            return Err(SwapError::Quota);
        }
        let mut digests = index_budget.try_buffer(count, [0_u8; 32])?;
        let scratch_len =
            usize::try_from(bytes.min(BLOCK as u64)).map_err(|_| SwapError::Length)?;
        let mut scratch = scratch_budget.try_buffer(scratch_len, 0_u8)?;
        let blob = self.write_stream(bytes, &mut cancelled, |writer| {
            let mut remaining = bytes;
            for digest in digests.iter_mut() {
                let length = usize::try_from(remaining.min(BLOCK as u64))
                    .map_err(|_| std::io::Error::other(SwapError::Length))?;
                reader.read_exact(&mut scratch[..length])?;
                writer.write_all(&scratch[..length])?;
                *digest = *blake3::hash(&scratch[..length]).as_bytes();
                remaining -= length as u64;
            }
            Ok(())
        })?;
        let mut trailing = [0];
        if reader.read(&mut trailing)? != 0 {
            return Err(SwapError::Length);
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        Ok(SegmentedSwapHandle(Arc::new(Index {
            blob,
            digests: digests.freeze(),
        })))
    }

    /// Read intersecting blocks, verifying each complete block before copying.
    /// Publishes only after all needed blocks pass integrity/cancellation. Managed
    /// RAM is result bytes plus at most one64KiB block, with index credit retained
    /// separately by the handle. Does not decode images or imply viewer streaming.
    ///
    /// # Errors
    /// Rejects foreign handles, invalid ranges, memory admission, cancellation and corrupt/I/O data.
    pub fn restore_range(
        &self,
        handle: &SegmentedSwapHandle,
        offset: u64,
        length: u64,
        output_budget: &MemoryBudget,
        scratch_budget: &MemoryBudget,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SharedBuffer<u8>> {
        self.restore_rows(
            handle,
            SwapRows {
                offset,
                row_bytes: length,
                stride: length,
                rows: 1,
            },
            output_budget,
            scratch_budget,
            cancelled,
        )
    }

    /// Restore packed rows from a strided source. Opens one file and reuses the
    /// last verified block across adjacent rows, without reading gaps. Rows must
    /// not overlap. Only requested blocks are verified. Output is published after
    /// all rows succeed; allocation/overflow/cancellation errors release buffers.
    ///
    /// # Errors
    /// Rejects foreign handles, invalid/overflowing layouts, memory admission, cancellation and corrupt/I/O data.
    pub fn restore_rows(
        &self,
        handle: &SegmentedSwapHandle,
        layout: SwapRows,
        output_budget: &MemoryBudget,
        scratch_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SharedBuffer<u8>> {
        if !Arc::ptr_eq(&self.0, &handle.0.blob.0.reservation.state) {
            return Err(SwapError::ForeignHandle);
        }
        let SwapRows {
            offset,
            row_bytes,
            stride,
            rows,
        } = layout;
        if stride < row_bytes {
            return Err(SwapError::Length);
        }
        let total = row_bytes.checked_mul(rows).ok_or(SwapError::Length)?;
        let end = if total == 0 {
            Some(offset)
        } else {
            (rows - 1)
                .checked_mul(stride)
                .and_then(|span| offset.checked_add(span))
                .and_then(|last| last.checked_add(row_bytes))
        }
        .filter(|end| *end <= handle.byte_len())
        .ok_or(SwapError::Length)?;
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let mut output = output_budget
            .try_buffer(usize::try_from(total).map_err(|_| SwapError::Length)?, 0_u8)?;
        if total == 0 {
            if cancelled() {
                return Err(SwapError::Cancelled);
            }
            return Ok(output.freeze());
        }
        let mut reader = self.segmented_reader(handle, scratch_budget, &mut cancelled)?;
        let width = usize::try_from(row_bytes).map_err(|_| SwapError::Length)?;
        for row in 0..rows {
            let position = offset + row * stride;
            debug_assert!(position + row_bytes <= end);
            let target = usize::try_from(row).map_err(|_| SwapError::Length)? * width;
            reader.read_into(
                position,
                &mut output[target..target + width],
                &mut cancelled,
            )?;
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        Ok(output.freeze())
    }
}

struct IndexedWriter<'a> {
    writer: &'a mut dyn Write,
    digests: &'a mut [[u8; 32]],
    hasher: blake3::Hasher,
    used: usize,
    index: usize,
}
impl Write for IndexedWriter<'_> {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        let count = self
            .writer
            .write(&input[..input.len().min(BLOCK - self.used)])?;
        self.hasher.update(&input[..count]);
        self.used += count;
        if self.used == BLOCK {
            let digest = self
                .digests
                .get_mut(self.index)
                .ok_or_else(|| std::io::Error::other(SwapError::Length))?;
            *digest = *self.hasher.finalize().as_bytes();
            self.hasher.reset();
            self.used = 0;
            self.index += 1;
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SwapLimits;
    use std::io::{Cursor, Write};

    #[test]
    #[ignore = "manual warm-filesystem scoped reader timing comparison"]
    fn scoped_reader_row_timing_comparison() {
        let directory = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0_u32..8 * 1024 * 1024)
            .map(|i| (i * 73).to_le_bytes()[0])
            .collect();
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(1),
            },
        )
        .unwrap();
        let index = MemoryBudget::new(4096);
        let scratch = MemoryBudget::new(BLOCK as u64);
        let output = MemoryBudget::new(2048);
        let handle = store
            .write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                &index,
                &scratch,
                || false,
            )
            .unwrap();
        // ABBA order counterbalances warm filesystem and scheduling effects.
        // Source and comparison storage are outside the managed API budgets.
        let mut restored = vec![0u8; bytes.len()];
        for block in 0..7 {
            for scoped in [false, true, true, false] {
                let start = std::time::Instant::now();
                if scoped {
                    let mut reader = store.segmented_reader(&handle, &scratch, || false).unwrap();
                    let mut row = output.try_buffer(2048, 0_u8).unwrap();
                    for (offset, destination) in
                        restored.as_chunks_mut::<2048>().0.iter_mut().enumerate()
                    {
                        reader
                            .read_into((offset * 2048) as u64, &mut row, || false)
                            .unwrap();
                        destination.copy_from_slice(&row);
                    }
                } else {
                    for (offset, destination) in
                        restored.as_chunks_mut::<2048>().0.iter_mut().enumerate()
                    {
                        let row = store
                            .restore_range(
                                &handle,
                                (offset * 2048) as u64,
                                2048,
                                &output,
                                &scratch,
                                || false,
                            )
                            .unwrap();
                        destination.copy_from_slice(&row);
                    }
                }
                let elapsed = start.elapsed();
                // Compare every byte after timing, including allocation teardown above.
                assert_eq!(restored, bytes);
                assert_eq!((output.used(), scratch.used()), (0, 0));
                eprintln!(
                    "warm row ABBA block={block}; measured={}; scoped={scoped}; ns={}; 8MiB;4096rows; all bytes exact; output=0;scratch=0",
                    block >= 2,
                    elapsed.as_nanos()
                );
            }
        }
        drop(handle);
        assert_eq!(
            (
                output.used(),
                scratch.used(),
                index.used(),
                store.usage().unwrap().bytes
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn scoped_reader_reuses_verified_bytes_and_invalidates_failed_block() {
        let directory = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..BLOCK * 2).map(|i| (i * 73).to_le_bytes()[0]).collect();
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(1),
            },
        )
        .unwrap();
        let index = MemoryBudget::new(64);
        let scratch = MemoryBudget::new(BLOCK as u64);
        let handle = store
            .write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                &index,
                &scratch,
                || false,
            )
            .unwrap();
        let mut reader = store.segmented_reader(&handle, &scratch, || false).unwrap();
        let mut out = [0; 16];
        reader.read_into(0, &mut out, || false).unwrap();
        let mut file = handle.0.blob.0.file.reopen().unwrap();
        file.write_all(&[bytes[0] ^ 255]).unwrap();
        reader.read_into(0, &mut out, || false).unwrap();
        assert_eq!(out, bytes[..16]); // verified snapshot, no second disk read
        file.seek(SeekFrom::Start(BLOCK as u64)).unwrap();
        file.write_all(&[bytes[BLOCK] ^ 255]).unwrap();
        assert!(matches!(
            reader.read_into(BLOCK as u64, &mut out, || false),
            Err(SwapError::Corrupt)
        ));
        // A failed replacement must invalidate the old cached index.
        assert!(matches!(
            reader.read_into(0, &mut out, || false),
            Err(SwapError::Corrupt)
        ));
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&bytes[..=BLOCK]).unwrap();
        reader
            .read_into((BLOCK - 8) as u64, &mut out, || false)
            .unwrap();
        assert_eq!(out, bytes[BLOCK - 8..BLOCK + 8]);
        assert!(matches!(
            reader.read_into(0, &mut out, || true),
            Err(SwapError::Cancelled)
        ));
        // Cancel after reading a replacement block but before verifying it.
        let mut checks = 0;
        assert!(matches!(
            reader.read_into(0, &mut out, || {
                checks += 1;
                checks == 2
            }),
            Err(SwapError::Cancelled)
        ));
        assert!(reader.cached_index.is_none());
        // A subsequent request must verify disk again, including any intervening damage.
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&[bytes[0] ^ 255]).unwrap();
        assert!(matches!(
            reader.read_into(0, &mut out, || false),
            Err(SwapError::Corrupt)
        ));
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&bytes[..1]).unwrap();
        reader.read_into(0, &mut out, || false).unwrap();
        assert_eq!(out, bytes[..16]);
        // A short physical read must not leave a sequential-position shortcut
        // pointing at the requested start or the nominal block end.
        file.set_len(BLOCK as u64 + 8).unwrap();
        assert!(reader.read_into(BLOCK as u64, &mut out, || false).is_err());
        assert!(reader.next_file_offset.is_none());
        assert!(reader.cached_index.is_none());
        file.seek(SeekFrom::Start(BLOCK as u64)).unwrap();
        file.write_all(&bytes[BLOCK..]).unwrap();
        reader.read_into(BLOCK as u64, &mut out, || false).unwrap();
        assert_eq!(out, bytes[BLOCK..BLOCK + 16]);
        assert_eq!(reader.next_file_offset, Some(bytes.len() as u64));
        drop(handle);
        assert_eq!(store.usage().unwrap().bytes, bytes.len() as u64);
        drop(reader);
        assert_eq!(
            (scratch.used(), index.used(), store.usage().unwrap().bytes),
            (0, 0, 0)
        );
    }

    #[test]
    fn packed_rows_match_independent_source_slices_and_skip_unrequested_blocks() {
        let directory = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..BLOCK * 5 + 37)
            .map(|i| (i.wrapping_mul(73) ^ (i >> 9)).to_le_bytes()[0])
            .collect();
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(1),
            },
        )
        .unwrap();
        let index = MemoryBudget::new(1024);
        let scratch = MemoryBudget::new(BLOCK as u64);
        let handle = store
            .write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                &index,
                &scratch,
                || false,
            )
            .unwrap();
        let output = MemoryBudget::new(4096);
        let mut cases = 0;
        for offset in [0, 1, BLOCK - 7, BLOCK + 3] {
            for width in [1, 7, 31, 64] {
                for rows in [0, 1, 3, 16] {
                    for stride in [width, width + 3, 2048] {
                        let layout = SwapRows {
                            offset: offset as u64,
                            row_bytes: width as u64,
                            stride: stride as u64,
                            rows: rows as u64,
                        };
                        let expected: Vec<u8> = (0..rows)
                            .flat_map(|row| {
                                bytes[offset + row * stride..offset + row * stride + width]
                                    .iter()
                                    .copied()
                            })
                            .collect();
                        let actual = store
                            .restore_rows(&handle, layout, &output, &scratch, || false)
                            .unwrap();
                        assert_eq!(&*actual, expected.as_slice(), "{layout:?}");
                        drop(actual);
                        assert_eq!((output.used(), scratch.used()), (0, 0));
                        cases += 1;
                    }
                }
            }
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(handle.0.blob.0.file.path())
            .unwrap();
        file.seek(SeekFrom::Start(BLOCK as u64)).unwrap();
        file.write_all(&[bytes[BLOCK] ^ 0xff]).unwrap();
        let layout = SwapRows {
            offset: 7,
            row_bytes: 8,
            stride: 2 * BLOCK as u64,
            rows: 3,
        };
        let actual = store
            .restore_rows(&handle, layout, &output, &scratch, || false)
            .unwrap();
        let expected: Vec<u8> = (0..3)
            .flat_map(|row| {
                bytes[7 + row * 2 * BLOCK..15 + row * 2 * BLOCK]
                    .iter()
                    .copied()
            })
            .collect();
        assert_eq!(&*actual, expected.as_slice());
        drop(actual);
        for bad in [
            SwapRows {
                stride: 7,
                ..layout
            },
            SwapRows {
                offset: u64::MAX,
                ..layout
            },
            SwapRows {
                row_bytes: u64::MAX,
                stride: u64::MAX,
                rows: 2,
                ..layout
            },
            SwapRows {
                stride: u64::MAX,
                ..layout
            },
        ] {
            assert!(matches!(
                store.restore_rows(&handle, bad, &output, &scratch, || false),
                Err(SwapError::Length)
            ));
        }
        assert_eq!((output.used(), scratch.used()), (0, 0));
        drop(handle);
        assert_eq!((index.used(), store.usage().unwrap().bytes), (0, 0));
        eprintln!(
            "strided swap: {cases} exact layouts; corrupted gap skipped; output/scratch/index/disk=0"
        );
    }

    #[test]
    fn generated_large_source_and_tiny_range_stay_within_one_block_plus_index() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = 8 * 1024 * 1024 + 1;
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes,
                max_objects: None,
            },
        )
        .unwrap();
        let index = MemoryBudget::new(8192);
        let scratch = MemoryBudget::new(BLOCK as u64);
        let mut source = std::io::repeat(0x5a).take(bytes);
        let handle = store
            .write_segmented(&mut source, bytes, &index, &scratch, || false)
            .unwrap();
        assert_eq!(handle.block_count(), 129);
        assert_eq!(store.usage().unwrap().objects, 1);
        let output = MemoryBudget::new(8);
        let actual = store
            .restore_range(&handle, bytes - 8, 8, &output, &scratch, || false)
            .unwrap();
        assert_eq!(&*actual, &[0x5a; 8]);
        assert_eq!(scratch.peak(), BLOCK as u64);
        assert!(index.peak() <= 8192);
        assert_eq!(output.peak(), 8);
        eprintln!(
            "segmented source={bytes} bytes objects={} index_peak={} scratch_peak={} output_peak={}",
            store.usage().unwrap().objects,
            index.peak(),
            scratch.peak(),
            output.peak()
        );
        drop(actual);
        drop(handle);
        assert_eq!(
            (
                index.used(),
                scratch.used(),
                output.used(),
                store.usage().unwrap().bytes
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn bounded_ranges_preserve_hdr_bits_verify_requested_blocks_and_release_owners() {
        let directory = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..BLOCK * 5 + 36)
            .map(|i| {
                // Includes negative zero, HDR, signed values and a NaN payload verbatim.
                [
                    0x8000_0000_u32,
                    8_f32.to_bits(),
                    (-2_f32).to_bits(),
                    0x7fc0_1234,
                ][(i / 4) % 4]
                    .to_le_bytes()[i % 4]
            })
            .collect();
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(6),
            },
        )
        .unwrap();
        let index = MemoryBudget::new(8192);
        let scratch = MemoryBudget::new(BLOCK as u64);
        let handle = store
            .write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                &index,
                &scratch,
                || false,
            )
            .unwrap();
        assert_eq!(handle.block_count(), 6);
        assert_eq!(store.usage().unwrap().bytes, bytes.len() as u64);
        assert_eq!(scratch.used(), 0);
        assert_eq!(scratch.peak(), BLOCK as u64);
        let output_budget = MemoryBudget::new(BLOCK as u64 + 19);
        let offset = BLOCK as u64 - 7;
        let result = store
            .restore_range(
                &handle,
                offset,
                BLOCK as u64 + 19,
                &output_budget,
                &scratch,
                || false,
            )
            .unwrap();
        assert_eq!(
            &*result,
            &bytes[usize::try_from(offset).unwrap()..usize::try_from(offset).unwrap() + BLOCK + 19]
        );
        assert_eq!(scratch.used(), 0);
        drop(result);
        assert_eq!(output_budget.used(), 0);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(handle.0.blob.0.file.path())
            .unwrap();
        file.seek(SeekFrom::Start((BLOCK * 5) as u64)).unwrap();
        file.write_all(&[0x55]).unwrap();
        // An unread corrupt block does not force a full-object scan.
        let good = store
            .restore_range(&handle, offset, 19, &output_budget, &scratch, || false)
            .unwrap();
        assert_eq!(
            &*good,
            &bytes[usize::try_from(offset).unwrap()..usize::try_from(offset).unwrap() + 19]
        );
        drop(good);
        assert!(matches!(
            store.restore_range(
                &handle,
                BLOCK as u64 * 5,
                36,
                &output_budget,
                &scratch,
                || false
            ),
            Err(SwapError::Corrupt)
        ));
        assert_eq!((output_budget.used(), scratch.used()), (0, 0));
        let retained = handle.clone();
        drop(handle);
        assert_eq!(store.usage().unwrap().objects, 1);
        assert!(index.used() > 0);
        drop(retained);
        assert_eq!(
            (
                index.used(),
                store.usage().unwrap().bytes,
                store.usage().unwrap().objects
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn failed_writes_ranges_and_cancellation_roll_back_all_credits() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = vec![42; BLOCK + 1];
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(0),
            },
        )
        .unwrap();
        let budget = MemoryBudget::new(BLOCK as u64 + 4096);
        assert!(
            store
                .write_segmented(
                    &mut Cursor::new(&bytes),
                    bytes.len() as u64,
                    &budget,
                    &budget,
                    || false
                )
                .is_err()
        );
        assert_eq!(
            (
                budget.used(),
                store.usage().unwrap().bytes,
                store.usage().unwrap().objects
            ),
            (0, 0, 0)
        );
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes.len() as u64,
                max_objects: Some(2),
            },
        )
        .unwrap();
        assert!(
            store
                .write_segmented(
                    &mut Cursor::new(&bytes[..BLOCK]),
                    bytes.len() as u64,
                    &budget,
                    &budget,
                    || false
                )
                .is_err()
        );
        assert!(matches!(
            store.write_segmented(
                &mut Cursor::new(&bytes),
                BLOCK as u64,
                &budget,
                &budget,
                || false
            ),
            Err(SwapError::Length)
        ));
        assert_eq!((budget.used(), store.usage().unwrap().bytes), (0, 0));
        let mut checks = 0;
        assert!(matches!(
            store.write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                &budget,
                &budget,
                || {
                    checks += 1;
                    checks == 4
                }
            ),
            Err(SwapError::Cancelled)
        ));
        assert_eq!((budget.used(), store.usage().unwrap().bytes), (0, 0));
        assert_range_rejections(&store, &bytes, &budget, directory.path());
    }

    fn assert_range_rejections(
        store: &SwapStore,
        bytes: &[u8],
        budget: &MemoryBudget,
        directory: &std::path::Path,
    ) {
        let handle = store
            .write_segmented(
                &mut Cursor::new(&bytes),
                bytes.len() as u64,
                budget,
                budget,
                || false,
            )
            .unwrap();
        let baseline = budget.used();
        let empty_budget = MemoryBudget::new(0);
        assert!(
            store
                .restore_range(
                    &handle,
                    handle.byte_len(),
                    0,
                    &empty_budget,
                    &empty_budget,
                    || false
                )
                .unwrap()
                .is_empty()
        );
        let output = MemoryBudget::new(32);
        let small_scratch = MemoryBudget::new(BLOCK as u64 - 1);
        assert!(matches!(
            store.restore_range(&handle, 0, 32, &output, &small_scratch, || false),
            Err(SwapError::Memory(_))
        ));
        assert!(matches!(
            store.restore_range(&handle, u64::MAX, 1, &output, budget, || false),
            Err(SwapError::Length)
        ));
        let foreign = SwapStore::new(
            directory,
            SwapLimits {
                max_bytes: 100,
                max_objects: None,
            },
        )
        .unwrap();
        assert!(matches!(
            foreign.restore_range(&handle, 0, 1, &output, budget, || false),
            Err(SwapError::ForeignHandle)
        ));
        let mut checks = 0;
        assert!(matches!(
            store.restore_range(&handle, 0, 32, &output, budget, || {
                checks += 1;
                checks == 4
            }),
            Err(SwapError::Cancelled)
        ));
        assert_eq!(
            (output.used(), small_scratch.used(), budget.used()),
            (0, 0, baseline)
        );
        assert_eq!(
            &*store
                .restore_range(&handle, 0, 32, &output, budget, || false)
                .unwrap(),
            &[42; 32]
        );
        drop(handle);
        assert_eq!((budget.used(), store.usage().unwrap().bytes), (0, 0));
    }
}
