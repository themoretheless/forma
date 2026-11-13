//! Device descriptors and device loss.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLostReason {
    Unknown,
    Destroyed,
}

#[derive(Debug, Clone, Default)]
pub struct AllocatorReport {
    pub total_allocated_bytes: u64,
    pub total_reserved_bytes: u64,
    pub allocations: Vec<()>,
    pub blocks: Vec<()>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryHints;

impl MemoryHints {
    #[allow(non_upper_case_globals)]
    pub const Performance: Self = Self;
    #[allow(non_upper_case_globals)]
    pub const MemoryUsage: Self = Self;
}

#[derive(Debug, Clone, Default)]
pub struct Trace;

impl Trace {
    #[allow(non_upper_case_globals)]
    pub const Off: Self = Self;
}

#[derive(Debug, Clone, Default)]
pub struct ExperimentalFeatures;

impl ExperimentalFeatures {
    pub const fn disabled() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct DeviceDescriptor<'a> {
    pub label: Label<'a>,
    pub required_features: Features,
    pub required_limits: Limits,
    pub experimental_features: ExperimentalFeatures,
    pub memory_hints: MemoryHints,
    pub trace: Trace,
}

impl Default for DeviceDescriptor<'_> {
    fn default() -> Self {
        Self {
            label: None,
            required_features: Features::empty(),
            required_limits: Limits::default(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: MemoryHints::Performance,
            trace: Trace::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestDeviceError(pub String);

impl std::fmt::Display for RequestDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RequestDeviceError {}
