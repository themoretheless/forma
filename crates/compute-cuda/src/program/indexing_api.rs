//! Typed recording follows the same data/count result contracts as eager calls.
use super::{CudaProgramBuilder, CudaValue};
use crate::CudaError;
use tensor_core::{Compacted, Gathered, ScanOptions, ScatterOp, Scattered};

macro_rules! scan {
    ($name:ident) => {
        pub fn $name(
            &mut self,
            value: CudaValue,
            axis: usize,
            options: ScanOptions,
        ) -> Result<CudaValue, CudaError> {
            self.plan.$name(value, axis, options)
        }
    };
}
macro_rules! gather {
    ($name:ident) => {
        pub fn $name(
            &mut self,
            value: CudaValue,
            indices: CudaValue,
            axis: usize,
        ) -> Result<Gathered<CudaValue, CudaValue>, CudaError> {
            self.plan.$name(value, indices, axis)
        }
    };
}
macro_rules! compact {
    ($name:ident) => {
        pub fn $name(
            &mut self,
            value: CudaValue,
            mask: CudaValue,
        ) -> Result<Compacted<CudaValue, CudaValue>, CudaError> {
            self.plan.$name(value, mask)
        }
    };
}
macro_rules! scatter {
    ($name:ident) => {
        pub fn $name(
            &mut self,
            value: CudaValue,
            indices: CudaValue,
            updates: CudaValue,
            op: ScatterOp,
            axis: usize,
        ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
            self.plan.$name(value, indices, updates, op, axis)
        }
    };
}
impl CudaProgramBuilder<'_> {
    scan!(scan);
    scan!(scan_u32);
    scan!(scan_low_f32);
    scan!(scan_low);
    gather!(gather);
    gather!(gather_u32);
    gather!(gather_low);
    compact!(compact);
    compact!(compact_u32);
    compact!(compact_low);
    scatter!(scatter);
    scatter!(scatter_u32);
    scatter!(scatter_low_f32);
    scatter!(scatter_low);
}
