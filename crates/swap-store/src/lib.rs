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
mod tests;

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod stream_tests;
