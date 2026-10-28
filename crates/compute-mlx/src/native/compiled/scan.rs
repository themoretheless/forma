use super::*;
use crate::native::lowering::{casts, scan};
use tensor_core::ScanOptions;

impl MlxProgramBuilder {
    /// Inclusive/exclusive f32 prefixes, optionally in reverse traversal order.
    pub fn scan(
        &mut self,
        value: MlxValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxValue, MlxError> {
        self.require(value, MlxDtype::F32)?;
        self.transaction(|graph| scan::scan(graph, value, axis, options))
    }
    /// Unsigned prefixes wrap modulo 2^32.
    pub fn scan_u32(
        &mut self,
        value: MlxValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxValue, MlxError> {
        self.require(value, MlxDtype::U32)?;
        self.transaction(|graph| scan::scan(graph, value, axis, options))
    }
    /// Decode low inputs directly and accumulate prefixes in f32.
    pub fn scan_low_f32(
        &mut self,
        value: MlxValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| scan::scan_low_f32(graph, value, axis, options))
    }
    /// Round each completed f32 prefix once to the input's low dtype.
    pub fn scan_low(
        &mut self,
        value: MlxValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let prefix = scan::scan_low_f32(graph, value, axis, options)?;
            casts::cast_to_low(graph, prefix, dtype)
        })
    }
}
