//! Shared production/mock state machine; the backend owns context/stream handles.
use super::CudaGraphError;
pub(super) trait Backend: Clone {
    type Graph: Copy + Default + Eq;
    type Exec: Copy + Default + Eq;
    fn bind(&self) -> Result<(), CudaGraphError>;
    fn bind_cleanup(&self) -> Result<(), CudaGraphError>;
    fn validate_stream(&self) -> Result<(), CudaGraphError>;
    fn is_capturing(&self) -> Result<bool, CudaGraphError>;
    fn begin(&self) -> Result<(), CudaGraphError>;
    // Keep output handles even on driver errors so partial results are owned.
    fn end(&self) -> (Self::Graph, Result<(), CudaGraphError>);
    fn instantiate(&self, graph: Self::Graph) -> (Self::Exec, Result<(), CudaGraphError>);
    fn upload(&self, exec: Self::Exec) -> Result<(), CudaGraphError>;
    fn launch(&self, exec: Self::Exec) -> Result<(), CudaGraphError>;
    fn destroy_graph(&self, graph: Self::Graph) -> Result<(), CudaGraphError>;
    fn destroy_exec(&self, exec: Self::Exec) -> Result<(), CudaGraphError>;
    fn cleanup_error(&self, error: CudaGraphError);
}
pub(super) struct Capture<B: Backend> {
    backend: B,
    active: bool,
}
impl<B: Backend> Capture<B> {
    pub fn begin(backend: B) -> Result<Self, CudaGraphError> {
        backend.bind()?;
        backend.validate_stream()?;
        backend.begin()?;
        Ok(Self {
            backend,
            active: true,
        })
    }
    fn end(&mut self) -> (B::Graph, Result<(), CudaGraphError>) {
        let result = self.backend.end();
        // Invalidated capture normally ends with an error and a null graph.
        // If another driver error leaves capture active, Drop retries cleanup.
        self.active = result.1.is_err() && self.backend.is_capturing().unwrap_or(true);
        result
    }
    pub fn finish(mut self) -> Result<Executable<B>, CudaGraphError> {
        self.backend.bind()?;
        let mut owned = Executable::empty(self.backend.clone());
        let (graph, result) = self.end();
        owned.graph = graph;
        if let Err(error) = result {
            // If end failed while capture remained active, drain it before
            // destroying a partial graph returned by that first call.
            drop(self);
            return Err(error);
        }
        if graph == B::Graph::default() {
            return Err(CudaGraphError::EmptyGraph);
        }
        let (exec, result) = self.backend.instantiate(graph);
        owned.exec = exec;
        result?;
        if exec == B::Exec::default() {
            return Err(CudaGraphError::EmptyGraph);
        }
        Ok(owned)
    }
    fn abort_inner(&mut self) -> Result<(), CudaGraphError> {
        if !self.active {
            return Ok(());
        }
        self.backend.bind_cleanup()?;
        let (graph, ended) = self.end();
        let retry_graph = if self.active {
            // A prior asynchronous error can prevent termination. Retry once;
            // retain partial handles until capture has actually ended.
            let (retry, result) = self.end();
            if let Err(error) = result {
                self.backend.cleanup_error(error);
            }
            retry
        } else {
            B::Graph::default()
        };
        if self.active {
            // The context/stream is unrecoverable here. Do not destroy a graph
            // which CUDA may still be building. The error reaches the caller or
            // retained context; normal invalidation always ends capture above.
            return ended;
        }
        let mut cleanup = Ok(());
        for handle in [graph, retry_graph] {
            if handle != B::Graph::default()
                && let Err(error) = self.backend.destroy_graph(handle)
            {
                self.backend.cleanup_error(error.clone());
                cleanup = Err(error);
            }
            if graph != B::Graph::default() && graph == retry_graph {
                break;
            }
        }
        ended.and(cleanup)
    }
    pub fn abort(mut self) -> Result<(), CudaGraphError> {
        self.abort_inner()
    }
}
impl<B: Backend> Drop for Capture<B> {
    fn drop(&mut self) {
        if let Err(error) = self.abort_inner() {
            self.backend.cleanup_error(error);
        }
    }
}
pub(super) struct Executable<B: Backend> {
    pub backend: B,
    graph: B::Graph,
    exec: B::Exec,
}
impl<B: Backend> Executable<B> {
    fn empty(backend: B) -> Self {
        Self {
            backend,
            graph: B::Graph::default(),
            exec: B::Exec::default(),
        }
    }
    pub fn upload(&mut self) -> Result<(), CudaGraphError> {
        self.backend.bind()?;
        self.backend.upload(self.exec)
    }
    pub fn launch(&mut self) -> Result<(), CudaGraphError> {
        self.backend.bind()?;
        self.backend.launch(self.exec)
    }
}
impl<B: Backend> Drop for Executable<B> {
    fn drop(&mut self) {
        if self.exec == B::Exec::default() && self.graph == B::Graph::default() {
            return;
        }
        if let Err(error) = self.backend.bind_cleanup() {
            // A failed context bind cannot safely destroy foreign-context handles.
            // Preserve the driver error for the owning context's next check_err.
            self.backend.cleanup_error(error);
            return;
        }
        let exec = std::mem::take(&mut self.exec);
        if exec != B::Exec::default()
            && let Err(error) = self.backend.destroy_exec(exec)
        {
            self.backend.cleanup_error(error);
        }
        let graph = std::mem::take(&mut self.graph);
        if graph != B::Graph::default()
            && let Err(error) = self.backend.destroy_graph(graph)
        {
            self.backend.cleanup_error(error);
        }
    }
}
