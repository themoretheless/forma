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
mod tests;
