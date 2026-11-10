//! Session-owned immutable swap blobs. Blocking I/O belongs on a caller's worker.
//! Handles retain disk quota and directory ownership; clones share one object.
//! This ephemeral store does not provide crash durability or evict dirty data.
//!
//! TTL and eviction policy belong to the composing cache. An immutable handle
//! keeps its disk quota until its last clone drops. Failed memory admission
//! does not consume or invalidate the handle, so restore can be retried.
//!
//! `write_segmented` stores independently verified64KiB blocks with a managed
//! digest index. `restore_range` reads only intersecting blocks and bounds
//! managed buffers to the result plus one block and retained index credit.
//! A segmented handle consumes one disk object; unread blocks are not reverified.
//! This byte-range primitive does not itself decode image tiles or wire them
//! into the viewer, and is not a crash-durable persistent format.
//! `restore_rows` packs non-overlapping source rows in one read operation,
//! reusing the last verified block and skipping blocks between requested rows.
//!
//! ```
//! use memory_budget::MemoryBudget;
//! use swap_store::{SwapStore, SwapLimits, SwapRows};
//! let directory = tempfile::tempdir()?;
//! let store = SwapStore::new(directory.path(), SwapLimits {max_bytes:8,max_objects:Some(1)})?;
//! let index = MemoryBudget::new(32);
//! let scratch = MemoryBudget::new(8);
//! let source = [1u8,2,3,4,5,6,7,8];
//! let handle = store.write_segmented(&mut std::io::Cursor::new(source),8,&index,&scratch,||false)?;
//! let output = MemoryBudget::new(4);
//! let selected = store.restore_rows(&handle,SwapRows {offset:1,row_bytes:2,stride:4,rows:2},
//!     &output,&scratch,||false)?;
//! assert_eq!(&*selected,&[2,3,6,7]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Composition contract
//! Disk quotas include writes in progress and objects retained by any handle.
//! RAM used by source buffers, restored buffers, and a composing write queue
//! has separate accounting; the disk quota does not bound those allocations.
//! Queue occupancy credits must not charge a shared pixel allocation a second
//! time against the same physical-memory budget.
//!
//! A cache coordinator chooses victims, TTL, write priorities and whether
//! restoring an object is preferable to decoding its source again. Blocking
//! writes and restores belong on workers, outside the render thread. Cancellation
//! is cooperative at stream I/O boundaries; a codec performing computation must
//! return to I/O before it can observe this store's cancellation callback.
//!
//! A persistent cache needs a separate identity/versioning and recovery
//! contract. These session handles do not promise survival after process exit.
//!
//! ```
//! use memory_budget::MemoryBudget;
//! use swap_store::{SwapStore, SwapLimits, SwapError};
//!
//! let directory = tempfile::tempdir()?;
//! let store = SwapStore::new(directory.path(), SwapLimits {
//!     max_bytes: 4, max_objects: Some(1),
//! })?;
//! let handle = store.write_chunks(4, [ &[1u8, 2, 3, 4][..] ], || false)?;
//! let retained = handle.clone();
//! assert_eq!(store.usage()?.objects, 1); // Clones do not duplicate quota.
//! assert!(matches!(store.restore(&handle, &MemoryBudget::new(3), || false),
//!                  Err(SwapError::Memory(_))));
//! let ram = MemoryBudget::new(4);
//! let restored = store.restore(&handle, &ram, || false)?;
//! assert_eq!(&*restored, &[1, 2, 3, 4]);
//! drop(handle);
//! assert_eq!(store.usage()?.bytes, 4); // Retained handle owns the disk object.
//! drop(retained);
//! assert_eq!(store.usage()?.bytes, 0);
//! assert_eq!(ram.used(), 4); // Restored RAM has independent ownership.
//! drop(restored);
//! assert_eq!(ram.used(), 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
use memory_budget::{MemoryBudget, MutableBuffer, SharedBuffer};
mod segmented;
pub use segmented::{SegmentedSwapHandle, SegmentedSwapReader, SwapRows};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tempfile::{NamedTempFile, TempDir};
use thiserror::Error;
const BLOCK: usize = 64 * 1024;
/// Maximum extra managed pixel-stream scratch across accelerated restores.
pub const PARALLEL_HASH_SCRATCH_BYTES: usize = 4 * 1024 * 1024;
static HASH_SLOT: AtomicBool = AtomicBool::new(false);
static HASH_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
struct HashPermit;
impl Drop for HashPermit {
    fn drop(&mut self) {
        HASH_SLOT.store(false, Ordering::Release);
    }
}
struct HashScratch {
    bytes: MutableBuffer<u8>,
    pool: &'static rayon::ThreadPool,
    _permit: HashPermit,
}
impl HashScratch {
    fn try_new(budget: &MemoryBudget) -> Option<Self> {
        HASH_SLOT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let permit = HashPermit;
        let pool = HASH_POOL
            .get_or_init(|| {
                let threads = std::thread::available_parallelism().ok()?.get().min(4);
                if threads < 2 {
                    return None;
                }
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .stack_size(512 * 1024)
                    .thread_name(|i| format!("rrrah-swap-hash-{i}"))
                    .build()
                    .ok()
            })
            .as_ref()?;
        let bytes = budget.try_buffer(PARALLEL_HASH_SCRATCH_BYTES, 0u8).ok()?;
        Some(Self {
            bytes,
            pool,
            _permit: permit,
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub struct SwapLimits {
    pub max_bytes: u64,
    pub max_objects: Option<usize>,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SwapUsage {
    pub bytes: u64,
    pub objects: usize,
}
#[derive(Debug, Error)]
pub enum SwapError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Memory(#[from] memory_budget::BufferError),
    #[error("swap disk quota exceeded")]
    Quota,
    #[error("swap operation cancelled")]
    Cancelled,
    #[error("swap length mismatch")]
    Length,
    #[error("swap digest mismatch")]
    Corrupt,
    #[error("swap handle belongs to another store")]
    ForeignHandle,
    #[error("swap state poisoned")]
    Poisoned,
}
type Result<T> = std::result::Result<T, SwapError>;
#[derive(Debug)]
struct State {
    directory: TempDir,
    limits: SwapLimits,
    usage: Mutex<SwapUsage>,
}
#[derive(Debug)]
struct Reservation {
    state: Arc<State>,
    bytes: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut usage = self
            .state
            .usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        usage.bytes -= self.bytes;
        usage.objects -= 1;
    }
}
#[derive(Debug)]
struct Entry {
    file: NamedTempFile,
    bytes: u64,
    digest: [u8; 32],
    reservation: Reservation,
}
#[derive(Debug, Clone)]
pub struct SwapHandle(Arc<Entry>);
impl SwapHandle {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn byte_len(&self) -> u64 {
        self.0.bytes
    }
    pub fn digest(&self) -> [u8; 32] {
        self.0.digest
    }
}
#[derive(Debug, Clone)]
pub struct SwapStore(Arc<State>);
impl SwapStore {
    /// Creates a private session directory under the supplied parent.
    ///
    /// # Errors
    /// Returns an I/O error if the temporary directory cannot be created.
    pub fn new(parent: &Path, limits: SwapLimits) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("rrrah-swap-")
            .tempdir_in(parent)?;
        Ok(Self(Arc::new(State {
            directory,
            limits,
            usage: Mutex::new(SwapUsage::default()),
        })))
    }
    /// Includes in-progress writes and blobs retained by any handle.
    ///
    /// # Errors
    /// Returns `SwapError::Poisoned` if the quota state was poisoned.
    pub fn usage(&self) -> Result<SwapUsage> {
        Ok(*self.0.usage.lock().map_err(|_| SwapError::Poisoned)?)
    }
    fn reserve(&self, bytes: u64) -> Result<Reservation> {
        let mut usage = self.0.usage.lock().map_err(|_| SwapError::Poisoned)?;
        if bytes > self.0.limits.max_bytes - usage.bytes
            || self
                .0
                .limits
                .max_objects
                .is_some_and(|max| usage.objects >= max)
        {
            return Err(SwapError::Quota);
        }
        let objects = usage.objects.checked_add(1).ok_or(SwapError::Quota)?;
        usage.bytes += bytes;
        usage.objects = objects;
        Ok(Reservation {
            state: Arc::clone(&self.0),
            bytes,
        })
    }
    /// Reserves declared length before I/O and publishes only a complete blob.
    /// Borrowed source chunks remain unchanged on cancellation or failure.
    ///
    /// # Errors
    /// Rejects exhausted disk quota, cancellation, wrong declared length and I/O failure.
    pub fn write_chunks<'a>(
        &self,
        bytes: u64,
        chunks: impl IntoIterator<Item = &'a [u8]>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SwapHandle> {
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let reservation = self.reserve(bytes)?;
        let mut file = NamedTempFile::new_in(self.0.directory.path())?;
        let mut hasher = blake3::Hasher::new();
        let mut written = 0_u64;
        for chunk in chunks {
            if cancelled() {
                return Err(SwapError::Cancelled);
            }
            for block in chunk.chunks(BLOCK) {
                if cancelled() {
                    return Err(SwapError::Cancelled);
                }
                written = written
                    .checked_add(u64::try_from(block.len()).map_err(|_| SwapError::Length)?)
                    .ok_or(SwapError::Length)?;
                if written > bytes {
                    return Err(SwapError::Length);
                }
                file.write_all(block)?;
                hasher.update(block);
            }
        }
        if written != bytes {
            return Err(SwapError::Length);
        }
        file.flush()?;
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        Ok(SwapHandle(Arc::new(Entry {
            file,
            bytes,
            digest: *hasher.finalize().as_bytes(),
            reservation,
        })))
    }
    /// Streams an encoder without retaining a complete serialized payload in RAM.
    ///
    /// # Errors
    /// Rejects quota exhaustion, cancellation, incorrect byte counts and encoder/I/O errors.
    pub fn write_stream(
        &self,
        bytes: u64,
        mut cancelled: impl FnMut() -> bool,
        encode: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<SwapHandle> {
        // Once observed, cancellation belongs to this whole operation even
        // when the caller's callback reports a transient signal.
        let mut observed = false;
        let mut check_cancelled = || {
            observed = observed || cancelled();
            observed
        };
        if check_cancelled() {
            return Err(SwapError::Cancelled);
        }
        let reservation = self.reserve(bytes)?;
        let mut file = NamedTempFile::new_in(self.0.directory.path())?;
        let mut hasher = blake3::Hasher::new();
        let mut remaining = bytes;
        let result = {
            #[expect(
                clippy::large_stack_arrays,
                reason = "Fixed 64 KiB I/O scratch stays on stack; heap scratch would require managed admission"
            )]
            let mut stream = WriteStream {
                buffer: [0; BLOCK],
                used: 0,
                file: file.as_file_mut(),
                hasher: &mut hasher,
                remaining: &mut remaining,
                cancelled: &mut check_cancelled,
            };
            encode(&mut stream).and_then(|()| stream.flush())
        };
        if check_cancelled() {
            return Err(SwapError::Cancelled);
        }
        result?;
        if remaining != 0 {
            return Err(SwapError::Length);
        }
        file.flush()?;
        Ok(SwapHandle(Arc::new(Entry {
            file,
            bytes,
            digest: *hasher.finalize().as_bytes(),
            reservation,
        })))
    }
    /// Publishes the decoder result only after consuming and verifying the complete blob.
    ///
    /// # Errors
    /// Rejects foreign handles, cancellation, incomplete reads, corrupt data and decoder/I/O errors.
    pub fn read_stream<T>(
        &self,
        handle: &SwapHandle,
        cancelled: impl FnMut() -> bool,
        decode: impl FnOnce(&mut dyn Read) -> std::io::Result<T>,
    ) -> Result<T> {
        self.read_stream_impl(handle, cancelled, decode, None)
    }
    /// Like `read_stream`, with opportunistic bounded parallel integrity hashing.
    /// A single process-wide four-thread pool and scratch slot are shared across
    /// stores. At most 4 MiB of extra scratch is charged to `scratch_budget`.
    /// Allocation/slot/pool failure falls back to the serial path without waiting
    /// for another restore. Scratch admission is deferred until the codec has
    /// consumed its first block, so pixel admission retains priority.
    /// Callers opting in must ensure future codec allocations are bounded by
    /// unread wire bytes; compressed/expanding codecs should use `read_stream`.
    /// Physical reads still poll cancellation every 64 KiB. A running hash batch
    /// is bounded to 4 MiB and joined before cancellation is checked again.
    /// Thread stacks (at most 4 × 512 KiB) and pool bookkeeping are outside the
    /// managed buffer budget. Both methods preserve identical object digests.
    ///
    /// # Errors
    /// Rejects foreign handles, cancellation, incomplete reads, corrupt data and decoder/I/O errors.
    pub fn read_stream_with_scratch<T>(
        &self,
        handle: &SwapHandle,
        cancelled: impl FnMut() -> bool,
        decode: impl FnOnce(&mut dyn Read) -> std::io::Result<T>,
        scratch_budget: &MemoryBudget,
    ) -> Result<T> {
        self.read_stream_impl(handle, cancelled, decode, Some(scratch_budget))
    }
    fn read_stream_impl<T>(
        &self,
        handle: &SwapHandle,
        mut cancelled: impl FnMut() -> bool,
        decode: impl FnOnce(&mut dyn Read) -> std::io::Result<T>,
        scratch_budget: Option<&MemoryBudget>,
    ) -> Result<T> {
        // Once observed, cancellation belongs to this whole operation even
        // when the caller's callback reports a transient signal.
        let mut observed = false;
        let mut check_cancelled = || {
            observed = observed || cancelled();
            observed
        };
        if !Arc::ptr_eq(&self.0, &handle.0.reservation.state) {
            return Err(SwapError::ForeignHandle);
        }
        if check_cancelled() {
            return Err(SwapError::Cancelled);
        }
        let mut file = handle.0.file.reopen()?;
        let mut hasher = blake3::Hasher::new();
        let mut remaining = handle.byte_len();
        let result = {
            #[expect(
                clippy::large_stack_arrays,
                reason = "Fixed 64 KiB I/O scratch stays on stack; heap scratch would require managed admission"
            )]
            let mut stream = ReadStream {
                buffer: [0; BLOCK],
                start: 0,
                end: 0,
                file: &mut file,
                hasher: &mut hasher,
                remaining: &mut remaining,
                cancelled: &mut check_cancelled,
                scratch_budget,
                scratch: None,
                attempted_scratch: false,
                delivered_block: false,
            };
            decode(&mut stream)
        };
        if check_cancelled() {
            return Err(SwapError::Cancelled);
        }
        let value = result?;
        if remaining != 0 {
            return Err(SwapError::Length);
        }
        let mut trailing = [0];
        if file.read(&mut trailing)? != 0 {
            return Err(SwapError::Length);
        }
        if hasher.finalize().as_bytes() != &handle.digest() {
            return Err(SwapError::Corrupt);
        }
        Ok(value)
    }
    /// Writes into caller-owned storage; publish the destination only after Ok.
    ///
    /// # Errors
    /// Rejects foreign handles, wrong lengths, cancellation, I/O failure and digest mismatch.
    pub fn read_into(
        &self,
        handle: &SwapHandle,
        destination: &mut [u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.0, &handle.0.reservation.state) {
            return Err(SwapError::ForeignHandle);
        }
        if u64::try_from(destination.len()).ok() != Some(handle.byte_len()) {
            return Err(SwapError::Length);
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let mut file = handle.0.file.reopen()?;
        let mut hasher = blake3::Hasher::new();
        for block in destination.chunks_mut(BLOCK) {
            if cancelled() {
                return Err(SwapError::Cancelled);
            }
            file.read_exact(block)?;
            hasher.update(block);
        }
        let mut trailing = [0];
        if file.read(&mut trailing)? != 0 {
            return Err(SwapError::Length);
        }
        if hasher.finalize().as_bytes() != &handle.0.digest {
            return Err(SwapError::Corrupt);
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        Ok(())
    }
    /// Reserves managed RAM before reading and returns immutable data only after verification.
    ///
    /// # Errors
    /// Returns memory admission errors or any error from `read_into`.
    pub fn restore(
        &self,
        handle: &SwapHandle,
        budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SharedBuffer<u8>> {
        if !Arc::ptr_eq(&self.0, &handle.0.reservation.state) {
            return Err(SwapError::ForeignHandle);
        }
        if cancelled() {
            return Err(SwapError::Cancelled);
        }
        let length = usize::try_from(handle.byte_len()).map_err(|_| SwapError::Length)?;
        let mut destination = budget.try_buffer(length, 0_u8)?;
        self.read_into(handle, &mut destination, cancelled)?;
        Ok(destination.freeze())
    }
}

struct WriteStream<'a> {
    buffer: [u8; BLOCK],
    used: usize,
    file: &'a mut std::fs::File,
    hasher: &'a mut blake3::Hasher,
    remaining: &'a mut u64,
    cancelled: &'a mut dyn FnMut() -> bool,
}
impl Write for WriteStream<'_> {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        if (self.cancelled)() {
            return Err(std::io::Error::other(SwapError::Cancelled));
        }
        let count = input.len().min(BLOCK);
        if u64::try_from(count).unwrap() > *self.remaining {
            return Err(std::io::Error::other(SwapError::Length));
        }
        let accepted = count.min(BLOCK - self.used);
        self.buffer[self.used..self.used + accepted].copy_from_slice(&input[..accepted]);
        self.used += accepted;
        *self.remaining -= u64::try_from(accepted).unwrap();
        if self.used == BLOCK {
            self.flush()?;
        }
        Ok(accepted)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if (self.cancelled)() {
            return Err(std::io::Error::other(SwapError::Cancelled));
        }
        if self.used > 0 {
            self.file.write_all(&self.buffer[..self.used])?;
            self.hasher.update(&self.buffer[..self.used]);
            self.used = 0;
        }
        self.file.flush()
    }
}
struct ReadStream<'a> {
    buffer: [u8; BLOCK],
    start: usize,
    end: usize,
    file: &'a mut std::fs::File,
    hasher: &'a mut blake3::Hasher,
    remaining: &'a mut u64,
    cancelled: &'a mut dyn FnMut() -> bool,
    scratch_budget: Option<&'a MemoryBudget>,
    scratch: Option<HashScratch>,
    attempted_scratch: bool,
    delivered_block: bool,
}
impl Read for ReadStream<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if (self.cancelled)() {
            return Err(std::io::Error::other(SwapError::Cancelled));
        }
        if *self.remaining == 0 {
            return Ok(0);
        }
        if self.start == self.end {
            if self.delivered_block
                && !self.attempted_scratch
                && *self.remaining >= PARALLEL_HASH_SCRATCH_BYTES as u64
            {
                self.attempted_scratch = true;
                // Leave at least the unread wire size for codecs that allocate
                // pixels after reading a large profile/header. Supported cache
                // payloads need no more than that for their remaining buffers.
                self.scratch = self
                    .scratch_budget
                    .filter(|budget| {
                        budget.available_bytes()
                            >= self
                                .remaining
                                .saturating_add(PARALLEL_HASH_SCRATCH_BYTES as u64)
                    })
                    .and_then(HashScratch::try_new);
            }
            if let Some(scratch) = self.scratch.as_mut() {
                let capacity =
                    usize::try_from((*self.remaining).min(scratch.bytes.len() as u64)).unwrap();
                self.start = 0;
                self.end = 0;
                for block in scratch.bytes[..capacity].chunks_mut(BLOCK) {
                    if (self.cancelled)() {
                        return Err(std::io::Error::other(SwapError::Cancelled));
                    }
                    self.file.read_exact(block)?;
                    self.end += block.len();
                }
                if (self.cancelled)() {
                    return Err(std::io::Error::other(SwapError::Cancelled));
                }
                scratch.pool.install(|| {
                    self.hasher.update_rayon(&scratch.bytes[..self.end]);
                });
                if (self.cancelled)() {
                    return Err(std::io::Error::other(SwapError::Cancelled));
                }
            } else {
                let capacity = usize::try_from((*self.remaining).min(BLOCK as u64)).unwrap();
                self.end = self.file.read(&mut self.buffer[..capacity])?;
                self.start = 0;
                if self.end == 0 {
                    return Ok(0);
                }
                self.hasher.update(&self.buffer[..self.end]);
            }
            self.delivered_block = true;
        }
        let count = output.len().min(self.end - self.start);
        let source: &[u8] = self
            .scratch
            .as_ref()
            .map_or(&self.buffer[..], |scratch| &scratch.bytes);
        output[..count].copy_from_slice(&source[self.start..self.start + count]);
        self.start += count;
        // Hash each fetched block once, but count only delivered bytes as
        // consumed. Exact-length validation still rejects a partial decoder.
        *self.remaining -= u64::try_from(count).unwrap();
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_small_and_bulk_restore_keeps_prefix_tail_and_hash_exact() {
        let source: Vec<u8> = (0..BLOCK * 4 + 97)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let store = make_store(source.len() as u64, Some(1));
        let handle = store
            .write_stream(
                source.len() as u64,
                || false,
                |writer| writer.write_all(&source),
            )
            .unwrap();
        let restored = store
            .read_stream(
                &handle,
                || false,
                |reader| {
                    let mut prefix = [0u8; 17];
                    reader.read_exact(&mut prefix)?;
                    let mut bulk = vec![0u8; BLOCK * 3];
                    reader.read_exact(&mut bulk)?;
                    let mut tail = Vec::new();
                    reader.read_to_end(&mut tail)?;
                    let mut result = prefix.to_vec();
                    result.extend(bulk);
                    result.extend(tail);
                    Ok(result)
                },
            )
            .unwrap();
        assert_eq!(restored, source);
    }

    fn make_store(bytes: u64, objects: Option<usize>) -> SwapStore {
        SwapStore::new(
            std::env::temp_dir().as_path(),
            SwapLimits {
                max_bytes: bytes,
                max_objects: objects,
            },
        )
        .unwrap()
    }
    #[test]
    fn streaming_cancellation_stays_cancelled_after_a_single_signal() {
        let store = make_store(8, Some(2));
        for swallow in [false, true] {
            let mut polls = 0;
            let result = store.write_stream(
                4,
                || {
                    polls += 1;
                    polls == 2
                },
                |writer| {
                    let result = writer.write_all(&[1, 2, 3, 4]);
                    if swallow { Ok(()) } else { result }
                },
            );
            assert!(matches!(result, Err(SwapError::Cancelled)));
            assert_eq!(
                store.usage().unwrap(),
                SwapUsage {
                    bytes: 0,
                    objects: 0
                }
            );
        }
        let handle = store
            .write_chunks(4, [&[1, 2, 3, 4][..]], || false)
            .unwrap();
        for swallow in [false, true] {
            let mut polls = 0;
            let result = store.read_stream(
                &handle,
                || {
                    polls += 1;
                    polls == 2
                },
                |reader| {
                    let mut output = [0; 4];
                    let result = reader.read_exact(&mut output);
                    if swallow {
                        Ok(output)
                    } else {
                        result.map(|()| output)
                    }
                },
            );
            assert!(matches!(result, Err(SwapError::Cancelled)));
            assert_eq!(
                store.usage().unwrap(),
                SwapUsage {
                    bytes: 4,
                    objects: 1
                }
            );
        }
        assert_eq!(
            &*store
                .restore(&handle, &MemoryBudget::new(4), || false)
                .unwrap(),
            &[1, 2, 3, 4]
        );
        drop(handle);
        assert_eq!(
            store.usage().unwrap(),
            SwapUsage {
                bytes: 0,
                objects: 0
            }
        );
    }
    #[test]
    fn roundtrip_shared_handles_and_buffers_retain_their_own_quotas() {
        let store = make_store(8, Some(1));
        let source = [1, 2, 3, 4, 5, 6, 7, 8];
        let handle = store
            .write_chunks(8, [&source[..3], &source[3..]], || false)
            .unwrap();
        let other = handle.clone();
        assert_eq!(
            store.usage().unwrap(),
            SwapUsage {
                bytes: 8,
                objects: 1
            }
        );
        assert!(matches!(
            store.write_chunks(0, [&[][..]], || false),
            Err(SwapError::Quota)
        ));
        let budget = MemoryBudget::new(8);
        let restored = store.restore(&handle, &budget, || false).unwrap();
        assert_eq!(&*restored, source);
        let consumer = restored.clone();
        drop(restored);
        assert_eq!(budget.used(), 8);
        let path = handle.0.file.path().to_path_buf();
        drop(handle);
        assert!(path.exists());
        assert_eq!(store.usage().unwrap().bytes, 8);
        drop(other);
        assert!(!path.exists());
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
        assert_eq!(&*consumer, source);
        drop(consumer);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn cancellation_failed_length_and_ram_failure_leave_source_and_usage_intact() {
        let store = make_store(200_000, None);
        let source = vec![42; 150_000];
        let mut calls = 0;
        assert!(matches!(
            store.write_chunks(150_000, [source.as_slice()], || {
                calls += 1;
                calls > 3
            }),
            Err(SwapError::Cancelled)
        ));
        assert_eq!(source, vec![42; 150_000]);
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
        for bytes in [149_999, 150_001] {
            assert!(matches!(
                store.write_chunks(bytes, [source.as_slice()], || false),
                Err(SwapError::Length)
            ));
            assert_eq!(store.usage().unwrap(), SwapUsage::default());
        }
        let handle = store
            .write_chunks(150_000, [source.as_slice()], || false)
            .unwrap();
        let small = MemoryBudget::new(1);
        assert!(matches!(
            store.restore(&handle, &small, || false),
            Err(SwapError::Memory(_))
        ));
        assert_eq!(small.used(), 0);
        let budget = MemoryBudget::new(150_000);
        let mut calls = 0;
        assert!(matches!(
            store.restore(&handle, &budget, || {
                calls += 1;
                calls > 2
            }),
            Err(SwapError::Cancelled)
        ));
        assert_eq!(budget.used(), 0);
        assert_eq!(store.usage().unwrap().bytes, 150_000);
    }
    #[test]
    fn corruption_truncation_trailing_and_foreign_handles_reject() {
        let store = make_store(16, None);
        let other = make_store(16, None);
        let handle = store
            .write_chunks(4, [&[1, 2, 3, 4][..]], || false)
            .unwrap();
        let budget = MemoryBudget::new(4);
        assert!(matches!(
            other.restore(&handle, &budget, || false),
            Err(SwapError::ForeignHandle)
        ));
        assert_eq!(budget.used(), 0);
        std::fs::write(handle.0.file.path(), [1, 2, 3, 9]).unwrap();
        assert!(matches!(
            store.restore(&handle, &budget, || false),
            Err(SwapError::Corrupt)
        ));
        assert_eq!(budget.used(), 0);
        std::fs::write(handle.0.file.path(), [1, 2, 3]).unwrap();
        assert!(matches!(
            store.restore(&handle, &budget, || false),
            Err(SwapError::Io(_))
        ));
        assert_eq!(budget.used(), 0);
        std::fs::write(handle.0.file.path(), [1, 2, 3, 4, 5]).unwrap();
        assert!(matches!(
            store.restore(&handle, &budget, || false),
            Err(SwapError::Length)
        ));
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn handles_keep_store_alive_and_zero_count_refuses_admission() {
        let store = make_store(0, Some(1));
        let handle = store.write_chunks(0, [&[][..]], || false).unwrap();
        let root = store.0.directory.path().to_path_buf();
        drop(store);
        assert!(root.exists());
        drop(handle);
        assert!(!root.exists());
        assert!(matches!(
            make_store(0, Some(0)).write_chunks(0, [&[][..]], || false),
            Err(SwapError::Quota)
        ));
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    #[test]
    fn concurrent_writes_share_disk_quota_and_reservations_roll_back() {
        let store = SwapStore::new(
            std::env::temp_dir().as_path(),
            SwapLimits {
                max_bytes: 8,
                max_objects: Some(2),
            },
        )
        .unwrap();
        let gate = Arc::new(std::sync::Barrier::new(8));
        let successes = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let store = store.clone();
                let gate = gate.clone();
                let successes = &successes;
                scope.spawn(move || {
                    let result = store.write_chunks(4, [&[1, 2, 3, 4][..]], || false);
                    if result.is_ok() {
                        successes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    gate.wait();
                    assert!(store.usage().unwrap().bytes <= 8);
                    assert!(result.is_ok() || matches!(result, Err(SwapError::Quota)));
                    drop(result);
                });
            }
        });
        assert_eq!(successes.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
    }
    #[test]
    fn concurrent_restores_share_ram_cap_without_consuming_disk_object() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        let bytes = BLOCK as u64;
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: bytes,
                max_objects: Some(1),
            },
        )
        .unwrap();
        let source = vec![73u8; BLOCK];
        let handle = store.write_chunks(bytes, [&source[..]], || false).unwrap();
        let budget = MemoryBudget::new(bytes);
        let children: Vec<_> = (0..8).map(|_| budget.child(bytes)).collect();
        let start = std::sync::Barrier::new(8);
        let retained = std::sync::Barrier::new(8);
        let successes = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for child in &children {
                let store = &store;
                let handle = &handle;
                let budget = child;
                let start = &start;
                let retained = &retained;
                let successes = &successes;
                scope.spawn(move || {
                    start.wait();
                    let restored = store.restore(handle, budget, || false);
                    match &restored {
                        Ok(buffer) => {
                            assert!(buffer.iter().all(|sample| *sample == 73));
                            successes.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(error) => assert!(matches!(error, SwapError::Memory(_))),
                    }
                    // Keep the successful allocation live until all competing
                    // requests have observed its budget reservation.
                    retained.wait();
                    assert_eq!(store.usage().unwrap(), SwapUsage { bytes, objects: 1 });
                    drop(restored);
                });
            }
        });
        assert_eq!(successes.load(Ordering::Relaxed), 1);
        assert_eq!(budget.peak(), bytes);
        assert_eq!(budget.used(), 0);
        assert!(children.iter().all(|child| child.used() == 0));
        assert_eq!(children.iter().filter(|child| child.peak() != 0).count(), 1);
        // Local admission can fail after reserving its ancestor. That partial
        // reservation must roll back so an unrelated child can still restore.
        let too_small = budget.child(bytes - 1);
        assert!(matches!(
            store.restore(&handle, &too_small, || false),
            Err(SwapError::Memory(_))
        ));
        assert_eq!(too_small.used(), 0);
        assert_eq!(budget.used(), 0);
        let retry = store.restore(&handle, &budget, || false).unwrap();
        assert_eq!(&*retry, source.as_slice());
        drop(retry);
        drop(handle);
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn already_cancelled_restore_allocates_no_managed_memory() {
        let store = SwapStore::new(
            std::env::temp_dir().as_path(),
            SwapLimits {
                max_bytes: 1,
                max_objects: None,
            },
        )
        .unwrap();
        let handle = store.write_chunks(1, [&[42][..]], || false).unwrap();
        let budget = MemoryBudget::new(0);
        assert!(matches!(
            store.restore(&handle, &budget, || true),
            Err(SwapError::Cancelled)
        ));
        assert_eq!(budget.peak(), 0);
    }

    #[test]
    fn multiblock_restore_cancellation_releases_ram_and_preserves_retry() {
        let directory = tempfile::tempdir().unwrap();
        let length = 2 * BLOCK + 17;
        let source: Vec<u8> = (0..length)
            .map(|index| u8::try_from(index % 251 + 1).unwrap())
            .collect();
        let store = SwapStore::new(
            directory.path(),
            SwapLimits {
                max_bytes: length as u64,
                max_objects: Some(1),
            },
        )
        .unwrap();
        let handle = store
            .write_chunks(length as u64, [&source[..]], || false)
            .unwrap();
        let expected_usage = store.usage().unwrap();
        let budget = MemoryBudget::new(length as u64);

        // Exercise every post-admission checkpoint, including cancellation after
        // the final block and digest verification, before publishing the buffer.
        for checkpoint in 2..=6 {
            let mut polls = 0;
            let mut live_at_cancellation = false;
            let result = store.restore(&handle, &budget, || {
                polls += 1;
                if polls == checkpoint {
                    live_at_cancellation = budget.used() == length as u64;
                    true
                } else {
                    false
                }
            });
            assert!(
                matches!(result, Err(SwapError::Cancelled)),
                "checkpoint {checkpoint}"
            );
            assert!(live_at_cancellation);
            assert_eq!(budget.used(), 0);
            assert_eq!(store.usage().unwrap(), expected_usage);
            let restored = store.restore(&handle, &budget, || false).unwrap();
            assert_eq!(&*restored, &source);
            drop(restored);
            assert_eq!(budget.used(), 0);
        }
        drop(handle);
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    #[test]
    fn stream_roundtrip_rejects_partial_decoders_and_bad_encoders() {
        let store = SwapStore::new(
            std::env::temp_dir().as_path(),
            SwapLimits {
                max_bytes: 200_000,
                max_objects: None,
            },
        )
        .unwrap();
        let source: Vec<u8> = (0..150_000)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect();
        let handle = store
            .write_stream(150_000, || false, |writer| writer.write_all(&source))
            .unwrap();
        let result = store
            .read_stream(
                &handle,
                || false,
                |reader| {
                    let mut out = Vec::new();
                    reader.read_to_end(&mut out)?;
                    Ok(out)
                },
            )
            .unwrap();
        assert_eq!(result, source);
        assert!(matches!(
            store.read_stream(
                &handle,
                || false,
                |reader| {
                    let mut byte = [0];
                    reader.read_exact(&mut byte)?;
                    Ok(byte)
                }
            ),
            Err(SwapError::Length)
        ));
        // Read-ahead can fetch the whole file, but a codec consuming only one
        // byte must still fail exact-length validation.
        let tiny = store
            .write_stream(4, || false, |writer| writer.write_all(&[1, 2, 3, 4]))
            .unwrap();
        assert!(matches!(
            store.read_stream(
                &tiny,
                || false,
                |reader| {
                    let mut byte = [0];
                    reader.read_exact(&mut byte)?;
                    Ok(byte)
                }
            ),
            Err(SwapError::Length)
        ));
        let flushed = store
            .write_stream(
                5,
                || false,
                |writer| {
                    writer.write_all(&[1, 2])?;
                    writer.flush()?;
                    writer.flush()?;
                    writer.write_all(&[3, 4, 5])
                },
            )
            .unwrap();
        let restored = store
            .restore(&flushed, &MemoryBudget::new(5), || false)
            .unwrap();
        assert_eq!(&*restored, &[1, 2, 3, 4, 5]);
        assert_stream_rejections(&store, &handle);
    }

    fn assert_stream_rejections(store: &SwapStore, handle: &SwapHandle) {
        let before = store.usage().unwrap();
        assert!(
            store
                .write_stream(2, || false, |writer| writer.write_all(&[1]))
                .is_err()
        );
        assert!(
            store
                .write_stream(1, || false, |writer| writer.write_all(&[1, 2]))
                .is_err()
        );
        assert_eq!(store.usage().unwrap(), before);
        let mut calls = 0;
        assert!(matches!(
            store.read_stream(
                handle,
                || {
                    calls += 1;
                    calls > 2
                },
                |reader| {
                    let mut out = Vec::new();
                    reader.read_to_end(&mut out)?;
                    Ok(out)
                }
            ),
            Err(SwapError::Cancelled)
        ));
        std::fs::write(handle.0.file.path(), vec![124; 150_000]).unwrap();
        assert!(matches!(
            store.read_stream(
                handle,
                || false,
                |reader| {
                    let mut out = Vec::new();
                    reader.read_to_end(&mut out)?;
                    Ok(out)
                }
            ),
            Err(SwapError::Corrupt)
        ));
    }
}
