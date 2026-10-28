//! Indexing launches borrow current resident bindings and retained scratch.
use super::{
    execution::Instruction,
    launch::source,
    operation::Operation,
    storage::{Storage, StorageMut, StorageRef},
    typed::CudaProgramInput,
};
use crate::{
    CudaError, CudaRuntime,
    indexing::{
        compact_dispatch::{CompactInputs, CompactOutputs},
        scan_dispatch::{LoadedScan, ScanOutputs},
        scatter_dispatch::{LoadedScatter, ScatterInputs},
    },
};

impl Instruction {
    pub(super) fn launch_indexing(
        &self,
        rt: &CudaRuntime,
        inputs: &[CudaProgramInput<'_>],
        scratch: &[Option<Storage>],
        output: StorageMut<'_>,
        auxiliary: Option<StorageMut<'_>>,
    ) -> Result<(), CudaError> {
        let metadata = || {
            self.metadata
                .as_ref()
                .expect("prepared indexing metadata exists")
        };
        let input = |id| source(id, inputs, scratch);
        match &self.operation {
            Operation::Zero => output.clear(rt),
            Operation::InvalidIndices { indices, pass } => {
                pass.launch(rt, input(*indices)?.u32()?, output.u32()?, metadata())
            }
            Operation::ScatterOwners { indices, pass } => {
                pass.launch(rt, input(*indices)?.u32()?, output.u32()?, metadata())
            }
            Operation::Normalize { mask, pass } => {
                pass.launch(rt, input(*mask)?.u32()?, output.u32()?, metadata())
            }
            Operation::Scan { source, pass, .. } => {
                let auxiliary = auxiliary.ok_or(CudaError::InvalidInput(
                    "scan totals destination is missing",
                ))?;
                match (input(*source)?, output, auxiliary) {
                    (StorageRef::F32(a), StorageMut::F32(values), StorageMut::F32(totals)) => pass
                        .launch(
                            rt,
                            LoadedScan {
                                function: &rt.indexing.scan[0],
                                dtype: None,
                            },
                            a,
                            ScanOutputs { values, totals },
                            metadata(),
                        ),
                    (StorageRef::U32(a), StorageMut::U32(values), StorageMut::U32(totals)) => pass
                        .launch(
                            rt,
                            LoadedScan {
                                function: &rt.indexing.scan[1],
                                dtype: None,
                            },
                            a,
                            ScanOutputs { values, totals },
                            metadata(),
                        ),
                    (StorageRef::Low(a, d), StorageMut::F32(values), StorageMut::F32(totals)) => {
                        pass.launch(
                            rt,
                            LoadedScan {
                                function: &rt.low.scan,
                                dtype: Some(d as u32),
                            },
                            a,
                            ScanOutputs { values, totals },
                            metadata(),
                        )
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::ScanCarry { offsets, pass } => match (input(*offsets)?, output) {
                (StorageRef::F32(a), StorageMut::F32(b)) => {
                    pass.launch(rt, &rt.indexing.add_scan[0], b, a)
                }
                (StorageRef::U32(a), StorageMut::U32(b)) => {
                    pass.launch(rt, &rt.indexing.add_scan[1], b, a)
                }
                _ => Err(CudaError::Dtype),
            },
            Operation::Gather {
                source,
                indices,
                pass,
            } => {
                let indices = input(*indices)?.u32()?;
                match (input(*source)?, output) {
                    (StorageRef::F32(a), StorageMut::F32(b)) => {
                        pass.launch(rt, &rt.indexing.gather[0], (a, indices), b, metadata())
                    }
                    (StorageRef::U32(a), StorageMut::U32(b)) => {
                        pass.launch(rt, &rt.indexing.gather[1], (a, indices), b, metadata())
                    }
                    (StorageRef::Low(a, d), StorageMut::Low(b, e)) if d == e => {
                        pass.launch(rt, &rt.low.gather, (a, indices), b, metadata())
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Compact {
                source,
                flags,
                prefix,
                pass,
                ..
            } => {
                let count = auxiliary
                    .ok_or(CudaError::InvalidInput(
                        "compaction count destination is missing",
                    ))?
                    .u32()?;
                let flags = input(*flags)?.u32()?;
                let prefix = input(*prefix)?.u32()?;
                match (input(*source)?, output) {
                    (StorageRef::F32(values), StorageMut::F32(out)) => pass.launch(
                        rt,
                        &rt.indexing.compact[0],
                        CompactInputs {
                            values,
                            flags,
                            prefix,
                        },
                        CompactOutputs { values: out, count },
                        metadata(),
                    ),
                    (StorageRef::U32(values), StorageMut::U32(out)) => pass.launch(
                        rt,
                        &rt.indexing.compact[1],
                        CompactInputs {
                            values,
                            flags,
                            prefix,
                        },
                        CompactOutputs { values: out, count },
                        metadata(),
                    ),
                    (StorageRef::Low(values, d), StorageMut::Low(out, e)) if d == e => pass.launch(
                        rt,
                        &rt.low.compact,
                        CompactInputs {
                            values,
                            flags,
                            prefix,
                        },
                        CompactOutputs { values: out, count },
                        metadata(),
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Scatter {
                indices,
                updates,
                owners,
                pass,
            } => {
                let indices = input(*indices)?.u32()?;
                let owners = input(*owners)?.u32()?;
                match (input(*updates)?, output) {
                    (StorageRef::F32(updates), StorageMut::F32(out)) => pass.launch(
                        rt,
                        LoadedScatter {
                            function: &rt.indexing.scatter[0],
                            dtype: None,
                        },
                        ScatterInputs {
                            indices,
                            updates,
                            owners,
                        },
                        out,
                        metadata(),
                    ),
                    (StorageRef::U32(updates), StorageMut::U32(out)) => pass.launch(
                        rt,
                        LoadedScatter {
                            function: &rt.indexing.scatter[1],
                            dtype: None,
                        },
                        ScatterInputs {
                            indices,
                            updates,
                            owners,
                        },
                        out,
                        metadata(),
                    ),
                    (StorageRef::Low(updates, d), StorageMut::Low(out, e)) if d == e => pass
                        .launch(
                            rt,
                            LoadedScatter {
                                function: &rt.low.scatter_raw,
                                dtype: None,
                            },
                            ScatterInputs {
                                indices,
                                updates,
                                owners,
                            },
                            out,
                            metadata(),
                        ),
                    (StorageRef::Low(updates, d), StorageMut::F32(out)) => pass.launch(
                        rt,
                        LoadedScatter {
                            function: &rt.low.scatter_f32,
                            dtype: Some(d as u32),
                        },
                        ScatterInputs {
                            indices,
                            updates,
                            owners,
                        },
                        out,
                        metadata(),
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            _ => Err(CudaError::InvalidInput(
                "invalid prepared indexing operation",
            )),
        }
    }
}
