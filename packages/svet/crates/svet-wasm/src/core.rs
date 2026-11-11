use svet_gpu_arena::GpuArena;
use svet_graph::ReactiveGraph;
use svet_reconcile::LisScratch;

/// Isolated linear-memory runtime state combining the reactive DAG, LIS reconciler,
/// GPU tensor arena, and shared `u32` FFI transfer buffers.
#[derive(Debug, Default)]
pub struct Core {
    pub graph: ReactiveGraph,
    pub lis: LisScratch,
    pub gpu: GpuArena,
    /// Shared input scratch buffer (`svet_prepare_lis_input` -> `svet_compute_lis_buffered` / `svet_sync_sources_buffered`).
    pub scratch_in: Vec<u32>,
    /// Shared output scratch buffer (`svet_get_lis_ptr` for LIS indices and DUMA dirty-span pairs).
    pub scratch_out: Vec<u32>,
}

impl Core {
    pub const fn new() -> Self {
        Self {
            graph: ReactiveGraph::new(),
            lis: LisScratch::new(),
            gpu: GpuArena::new(),
            scratch_in: Vec::new(),
            scratch_out: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.graph.reset();
        self.lis.reset();
        self.gpu.reset();
        self.scratch_in.clear();
        self.scratch_out.clear();
    }
}
