use super::{lifecycle::*, *};
use std::cell::RefCell;
#[derive(Default)]
struct State {
    calls: Vec<&'static str>,
    fail: Option<&'static str>,
    active: bool,
    end_stays_active_once: bool,
    graph: usize,
    exec: usize,
    errors: usize,
}
#[derive(Clone)]
struct Mock(Rc<RefCell<State>>);
impl Mock {
    fn new(fail: Option<&'static str>) -> Self {
        Self(Rc::new(RefCell::new(State {
            fail,
            graph: 1,
            exec: 2,
            ..State::default()
        })))
    }
    fn call(&self, op: &'static str) -> Result<(), CudaGraphError> {
        let mut s = self.0.borrow_mut();
        s.calls.push(op);
        if s.fail == Some(op) {
            Err(CudaGraphError::Driver {
                operation: op,
                code: sys::CUresult::CUDA_ERROR_INVALID_VALUE,
            })
        } else {
            Ok(())
        }
    }
    fn calls(&self) -> Vec<&'static str> {
        self.0.borrow().calls.clone()
    }
}
impl Backend for Mock {
    type Graph = usize;
    type Exec = usize;
    fn bind(&self) -> Result<(), CudaGraphError> {
        self.call("bind")
    }
    fn bind_cleanup(&self) -> Result<(), CudaGraphError> {
        self.call("cleanup_bind")
    }
    fn validate_stream(&self) -> Result<(), CudaGraphError> {
        self.call("validate")
    }
    fn is_capturing(&self) -> Result<bool, CudaGraphError> {
        self.call("status")?;
        Ok(self.0.borrow().active)
    }
    fn begin(&self) -> Result<(), CudaGraphError> {
        self.call("begin")?;
        self.0.borrow_mut().active = true;
        Ok(())
    }
    fn end(&self) -> (usize, Result<(), CudaGraphError>) {
        let r = self.call("end");
        let mut state = self.0.borrow_mut();
        state.active = r.is_err() && state.end_stays_active_once;
        let graph = state.graph;
        if state.end_stays_active_once {
            state.end_stays_active_once = false;
            state.fail = None;
            state.graph = 0;
        }
        (graph, r)
    }
    fn instantiate(&self, _: usize) -> (usize, Result<(), CudaGraphError>) {
        {
            let exec = self.0.borrow().exec;
            (exec, self.call("instantiate"))
        }
    }
    fn upload(&self, _: usize) -> Result<(), CudaGraphError> {
        self.call("upload")
    }
    fn launch(&self, _: usize) -> Result<(), CudaGraphError> {
        self.call("launch")
    }
    fn destroy_graph(&self, _: usize) -> Result<(), CudaGraphError> {
        self.call("destroy_graph")
    }
    fn destroy_exec(&self, _: usize) -> Result<(), CudaGraphError> {
        self.call("destroy_exec")
    }
    fn cleanup_error(&self, _: CudaGraphError) {
        self.0.borrow_mut().errors += 1;
    }
}
#[test]
fn success_upload_launch_and_destruction_have_one_owner_and_order() {
    let mock = Mock::new(None);
    let mut graph = Capture::begin(mock.clone()).unwrap().finish().unwrap();
    graph.upload().unwrap();
    graph.launch().unwrap();
    graph.launch().unwrap();
    drop(graph);
    assert_eq!(
        mock.calls(),
        [
            "bind",
            "validate",
            "begin",
            "bind",
            "end",
            "instantiate",
            "bind",
            "upload",
            "bind",
            "launch",
            "bind",
            "launch",
            "cleanup_bind",
            "destroy_exec",
            "destroy_graph"
        ]
    );
}
#[test]
fn abandoned_capture_and_rust_unwind_end_and_destroy_without_instantiation() {
    for panic in [false, true] {
        let mock = Mock::new(None);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _capture = Capture::begin(mock.clone()).unwrap();
            if panic {
                panic!("injected recording failure")
            }
        }));
        assert_eq!(result.is_err(), panic);
        assert_eq!(
            mock.calls(),
            [
                "bind",
                "validate",
                "begin",
                "cleanup_bind",
                "end",
                "destroy_graph"
            ]
        );
        assert!(!mock.0.borrow().active);
    }
}
#[test]
fn preflight_and_failed_begin_never_end_someone_elses_capture() {
    for fail in ["bind", "validate", "begin"] {
        let mock = Mock::new(Some(fail));
        assert!(Capture::begin(mock.clone()).is_err());
        assert!(!mock.calls().contains(&"end"));
        assert!(!mock.calls().contains(&"destroy_graph"));
    }
}
#[test]
fn invalidated_end_and_partial_graph_cleanup_are_not_skipped() {
    for graph in [0, 1] {
        let mock = Mock::new(Some("end"));
        mock.0.borrow_mut().graph = graph;
        assert!(Capture::begin(mock.clone()).unwrap().finish().is_err());
        assert!(!mock.calls().contains(&"instantiate"));
        assert_eq!(
            mock.calls()
                .iter()
                .filter(|&&x| x == "destroy_graph")
                .count(),
            usize::from(graph != 0)
        );
        assert!(!mock.0.borrow().active);
    }
}
#[test]
fn instantiate_failure_destroys_partial_exec_before_source_graph() {
    for exec in [0, 2] {
        let mock = Mock::new(Some("instantiate"));
        mock.0.borrow_mut().exec = exec;
        assert!(Capture::begin(mock.clone()).unwrap().finish().is_err());
        let calls = mock.calls();
        assert_eq!(calls.last(), Some(&"destroy_graph"));
        assert_eq!(
            calls.iter().filter(|&&x| x == "destroy_exec").count(),
            usize::from(exec != 0)
        );
        if exec != 0 {
            assert_eq!(&calls[calls.len() - 2..], ["destroy_exec", "destroy_graph"]);
        }
    }
}
#[test]
fn null_graph_or_exec_is_rejected_and_resources_cleaned() {
    for (graph, exec) in [(0, 2), (1, 0)] {
        let mock = Mock::new(None);
        {
            let mut s = mock.0.borrow_mut();
            s.graph = graph;
            s.exec = exec;
        }
        assert!(matches!(
            Capture::begin(mock.clone()).unwrap().finish(),
            Err(CudaGraphError::EmptyGraph)
        ));
        assert_eq!(mock.calls().contains(&"destroy_graph"), graph != 0);
    }
}
#[test]
fn abort_reports_end_error_and_cleanup_ignores_prior_bind_error() {
    let mock = Mock::new(Some("end"));
    assert!(Capture::begin(mock.clone()).unwrap().abort().is_err());
    assert_eq!(mock.calls().last(), Some(&"destroy_graph"));
    let mock = Mock::new(None);
    let capture = Capture::begin(mock.clone()).unwrap();
    mock.0.borrow_mut().fail = Some("bind");
    assert!(capture.finish().is_err());
    assert_eq!(mock.calls().last(), Some(&"destroy_graph"));
}
#[test]
fn launch_upload_and_exec_destroy_errors_do_not_leak_graph() {
    for fail in ["upload", "launch", "destroy_exec"] {
        let mock = Mock::new(Some(fail));
        let mut graph = Capture::begin(mock.clone()).unwrap().finish().unwrap();
        if fail == "upload" {
            assert!(graph.upload().is_err());
        } else if fail == "launch" {
            assert!(graph.launch().is_err());
        }
        drop(graph);
        assert_eq!(mock.calls().last(), Some(&"destroy_graph"));
        assert_eq!(mock.0.borrow().errors, usize::from(fail == "destroy_exec"));
    }
}
#[test]
fn absent_driver_is_explicit_and_error_formatting_needs_no_driver() {
    if !unsafe { sys::is_culib_present() } {
        assert!(matches!(
            CudaGraphApi::load(),
            Err(CudaGraphError::Unavailable)
        ));
    }
    let error = CudaGraphError::Driver {
        operation: "mock",
        code: sys::CUresult::CUDA_ERROR_INVALID_VALUE,
    };
    assert!(error.to_string().contains("mock"));
}

#[test]
fn end_error_that_leaves_capture_active_is_retried_before_partial_graph_destroy() {
    let mock = Mock::new(Some("end"));
    mock.0.borrow_mut().end_stays_active_once = true;
    assert!(Capture::begin(mock.clone()).unwrap().finish().is_err());
    assert_eq!(
        mock.calls(),
        [
            "bind",
            "validate",
            "begin",
            "bind",
            "end",
            "status",
            "cleanup_bind",
            "end",
            "cleanup_bind",
            "destroy_graph"
        ]
    );
    assert!(!mock.0.borrow().active);
}
#[test]
fn failed_cleanup_bind_records_error_without_using_handles_in_wrong_context() {
    let mock = Mock::new(None);
    let graph = Capture::begin(mock.clone()).unwrap().finish().unwrap();
    mock.0.borrow_mut().fail = Some("cleanup_bind");
    drop(graph);
    assert_eq!(mock.0.borrow().errors, 1);
    assert!(!mock.calls().contains(&"destroy_exec"));
    assert!(!mock.calls().contains(&"destroy_graph"));
}

#[test]
fn explicit_abort_drains_remaining_capture_before_destroying_partial_graph() {
    let mock = Mock::new(Some("end"));
    mock.0.borrow_mut().end_stays_active_once = true;
    assert!(Capture::begin(mock.clone()).unwrap().abort().is_err());
    assert_eq!(
        mock.calls(),
        [
            "bind",
            "validate",
            "begin",
            "cleanup_bind",
            "end",
            "status",
            "end",
            "destroy_graph"
        ]
    );
    assert!(!mock.0.borrow().active);
}
