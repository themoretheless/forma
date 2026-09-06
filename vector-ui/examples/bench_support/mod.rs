//! Low-overhead, process-local metrics for benchmark executables.
//!
//! The allocator counts successful Rust `GlobalAlloc` calls and requested bytes,
//! not Objective-C, Metal/driver, mmap, allocator bookkeeping, or physical pages.
//! `requested_bytes` is allocation traffic (a realloc contributes its new size),
//! while `live_bytes` is the size of currently live Rust allocations. In a
//! multithreaded process snapshots are observational, not atomic transactions.
//! Process CPU includes all threads; 100% means one fully occupied CPU core.
//! RSS includes the whole process, is not additive to reported GPU resources on
//! unified-memory machines, and should be sampled outside per-frame timings.

#![allow(dead_code)] // Different benchmark binaries use different metrics.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct CountingAllocator {
    allocations: AtomicU64,
    deallocations: AtomicU64,
    reallocations: AtomicU64,
    requested_bytes: AtomicU64,
    live_bytes: AtomicU64,
    peak_live_bytes: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationSnapshot {
    pub allocations: u64,
    pub deallocations: u64,
    pub reallocations: u64,
    pub requested_bytes: u64,
    pub live_bytes: u64,
    /// Absolute live-byte highwater since startup or the latest `reset_peak`.
    pub peak_live_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationDelta {
    pub allocations: u64,
    pub deallocations: u64,
    pub reallocations: u64,
    pub requested_bytes: u64,
    pub live_bytes_change: i128,
    /// Absolute, not a delta: memory retained before the phase is included.
    pub peak_live_bytes: u64,
}

impl AllocationSnapshot {
    pub fn delta_since(self, start: Self) -> AllocationDelta {
        AllocationDelta {
            allocations: self.allocations.saturating_sub(start.allocations),
            deallocations: self.deallocations.saturating_sub(start.deallocations),
            reallocations: self.reallocations.saturating_sub(start.reallocations),
            requested_bytes: self.requested_bytes.saturating_sub(start.requested_bytes),
            live_bytes_change: i128::from(self.live_bytes) - i128::from(start.live_bytes),
            peak_live_bytes: self.peak_live_bytes,
        }
    }
}

impl CountingAllocator {
    pub const fn new() -> Self {
        Self {
            allocations: AtomicU64::new(0),
            deallocations: AtomicU64::new(0),
            reallocations: AtomicU64::new(0),
            requested_bytes: AtomicU64::new(0),
            live_bytes: AtomicU64::new(0),
            peak_live_bytes: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> AllocationSnapshot {
        AllocationSnapshot {
            allocations: self.allocations.load(Ordering::Relaxed),
            deallocations: self.deallocations.load(Ordering::Relaxed),
            reallocations: self.reallocations.load(Ordering::Relaxed),
            requested_bytes: self.requested_bytes.load(Ordering::Relaxed),
            live_bytes: self.live_bytes.load(Ordering::Relaxed),
            peak_live_bytes: self.peak_live_bytes.load(Ordering::Relaxed),
        }
    }

    /// Start a new peak window without resetting live bytes or call counters.
    /// Call at a quiescent phase boundary: racing allocations may straddle it.
    pub fn reset_peak(&self) -> u64 {
        let live = self.live_bytes.load(Ordering::Relaxed);
        self.peak_live_bytes.store(live, Ordering::Relaxed);
        live
    }

    fn grow_live(&self, size: u64) {
        let live = self.live_bytes.fetch_add(size, Ordering::Relaxed) + size;
        self.peak_live_bytes.fetch_max(live, Ordering::Relaxed);
    }

    fn record_allocation(&self, size: usize) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        self.requested_bytes
            .fetch_add(size as u64, Ordering::Relaxed);
        self.grow_live(size as u64);
    }
}

// SAFETY: This wrapper delegates all allocation operations, with their original
// layouts and pointers, to System. Its accounting uses nonallocating atomics.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            self.record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            self.record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        self.deallocations.fetch_add(1, Ordering::Relaxed);
        self.live_bytes
            .fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        // A failed realloc leaves the old allocation alive, so changes nothing.
        if !new_pointer.is_null() {
            self.reallocations.fetch_add(1, Ordering::Relaxed);
            self.requested_bytes
                .fetch_add(new_size as u64, Ordering::Relaxed);
            if new_size >= layout.size() {
                self.grow_live((new_size - layout.size()) as u64);
            } else {
                self.live_bytes
                    .fetch_sub((layout.size() - new_size) as u64, Ordering::Relaxed);
            }
        }
        new_pointer
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessSnapshot {
    pub cpu_seconds: f64,
    /// Current resident memory, including allocations outside Rust's allocator.
    pub resident_bytes: u64,
    /// OS process-lifetime RSS highwater, not a per-phase peak or GPU residency.
    pub peak_resident_bytes: u64,
}

impl ProcessSnapshot {
    pub fn capture() -> io::Result<Self> {
        capture_process()
    }

    pub fn cpu_pct_one_core_since(self, start: Self, wall_seconds: f64) -> f64 {
        if wall_seconds > 0.0 {
            (self.cpu_seconds - start.cpu_seconds).max(0.0) / wall_seconds * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn capture_process() -> io::Result<ProcessSnapshot> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes one correctly aligned rusage; initialized only
    // after success. RUSAGE_SELF accounts for this process, including threads.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let usage = unsafe { usage.assume_init() };
    let cpu_seconds = usage.ru_utime.tv_sec as f64
        + usage.ru_utime.tv_usec as f64 / 1_000_000.0
        + usage.ru_stime.tv_sec as f64
        + usage.ru_stime.tv_usec as f64 / 1_000_000.0;
    // Darwin exposes bytes; Linux exposes KiB for ru_maxrss.
    #[cfg(target_os = "macos")]
    let peak_resident_bytes = usage.ru_maxrss.max(0) as u64;
    #[cfg(target_os = "linux")]
    let peak_resident_bytes = (usage.ru_maxrss.max(0) as u64).saturating_mul(1024);
    Ok(ProcessSnapshot {
        cpu_seconds,
        resident_bytes: current_resident_bytes()?,
        peak_resident_bytes,
    })
}

#[cfg(target_os = "macos")]
fn current_resident_bytes() -> io::Result<u64> {
    let mut task = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let expected = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: PROC_PIDTASKINFO's documented output is proc_taskinfo. The exact
    // buffer size is passed and validated before reading initialized contents.
    let copied = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            task.as_mut_ptr().cast(),
            expected,
        )
    };
    if copied <= 0 {
        return Err(io::Error::last_os_error());
    }
    if copied != expected {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short PROC_PIDTASKINFO result",
        ));
    }
    Ok(unsafe { task.assume_init() }.pti_resident_size)
}

#[cfg(target_os = "linux")]
fn current_resident_bytes() -> io::Result<u64> {
    use std::io::Read;
    // Fixed stack storage avoids allocating a String in the measurement helper.
    let mut buffer = [0u8; 256];
    let length = std::fs::File::open("/proc/self/statm")?.read(&mut buffer)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid /proc/self/statm");
    let text = std::str::from_utf8(&buffer[..length]).map_err(|_| invalid())?;
    let resident_pages = text
        .split_ascii_whitespace()
        .nth(1)
        .ok_or_else(invalid)?
        .parse::<u64>()
        .map_err(|_| invalid())?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(io::Error::last_os_error());
    }
    resident_pages
        .checked_mul(page_size as u64)
        .ok_or_else(invalid)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn capture_process() -> io::Result<ProcessSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process metrics implemented for macOS/Linux only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_counts_live_bytes_and_realloc_growth_and_shrink() {
        let allocator = CountingAllocator::new();
        let start = allocator.snapshot();
        let small = Layout::from_size_align(32, 8).unwrap();
        let large = Layout::from_size_align(128, 8).unwrap();
        let tiny = Layout::from_size_align(8, 8).unwrap();
        unsafe {
            let mut pointer = allocator.alloc(small);
            assert!(!pointer.is_null());
            assert_eq!(allocator.snapshot().live_bytes, 32);
            pointer = allocator.realloc(pointer, small, 128);
            assert!(!pointer.is_null());
            assert_eq!(allocator.snapshot().live_bytes, 128);
            pointer = allocator.realloc(pointer, large, 8);
            assert!(!pointer.is_null());
            let metrics = allocator.snapshot();
            assert_eq!(metrics.live_bytes, 8);
            assert_eq!(metrics.peak_live_bytes, 128);
            allocator.dealloc(pointer, tiny);
        }
        assert_eq!(
            allocator.snapshot().delta_since(start),
            AllocationDelta {
                allocations: 1,
                deallocations: 1,
                reallocations: 2,
                requested_bytes: 168,
                live_bytes_change: 0,
                peak_live_bytes: 128,
            }
        );
    }

    #[test]
    fn zeroed_allocations_and_peak_reset_preserve_live_accounting() {
        let allocator = CountingAllocator::new();
        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let first = allocator.alloc_zeroed(layout);
            assert!(!first.is_null());
            assert!(std::slice::from_raw_parts(first, 64)
                .iter()
                .all(|&byte| byte == 0));
            let second = allocator.alloc(layout);
            assert!(!second.is_null());
            allocator.dealloc(second, layout);
            assert_eq!(allocator.snapshot().peak_live_bytes, 128);
            assert_eq!(allocator.reset_peak(), 64);
            assert_eq!(allocator.snapshot().peak_live_bytes, 64);
            allocator.dealloc(first, layout);
        }
        assert_eq!(allocator.snapshot().live_bytes, 0);
        assert_eq!(allocator.snapshot().requested_bytes, 128);
    }

    #[test]
    fn cpu_percent_is_per_core_and_can_exceed_one_hundred() {
        let start = ProcessSnapshot {
            cpu_seconds: 1.0,
            resident_bytes: 0,
            peak_resident_bytes: 0,
        };
        let end = ProcessSnapshot {
            cpu_seconds: 3.0,
            ..start
        };
        assert_eq!(end.cpu_pct_one_core_since(start, 1.0), 200.0);
        assert_eq!(end.cpu_pct_one_core_since(start, 0.0), 0.0);
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn process_metrics_are_available_without_a_child_process() {
        let start = ProcessSnapshot::capture().unwrap();
        assert!(start.cpu_seconds.is_finite() && start.cpu_seconds >= 0.0);
        assert!(start.resident_bytes > 0);
        assert!(start.peak_resident_bytes > 0);
        let end = ProcessSnapshot::capture().unwrap();
        assert!(end.cpu_seconds >= start.cpu_seconds);
    }
}
