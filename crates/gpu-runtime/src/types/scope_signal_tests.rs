use super::*;
#[test]
fn completion_before_parking_skips_wait_even_when_generation_wraps() {
    let service = ScopeCompletionService::default();
    service.0.lock().unwrap().epoch = u64::MAX;
    let observed = u64::MAX;
    service.notify();
    assert_eq!(service.0.lock().unwrap().epoch, 0);
    let state = service.0.lock().unwrap();
    assert_eq!(
        service.park_if_unchanged(state, observed, std::time::Duration::from_secs(1)),
        ScopePark::GenerationChanged
    );
}
#[test]
fn notification_after_atomic_unlock_wakes_parked_service() {
    let service = Arc::new(ScopeCompletionService::default());
    let waiter = service.clone();
    let (ready, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let state = waiter.0.lock().unwrap();
        let observed = state.epoch;
        ready.send(()).unwrap();
        waiter.park_if_unchanged(state, observed, std::time::Duration::from_secs(1))
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    // notify must acquire the same mutex; it cannot race into the gap
    // between the generation check and Condvar's atomic unlock+park.
    service.notify();
    assert_eq!(thread.join().unwrap(), ScopePark::Awakened);
}
#[test]
fn absent_notification_has_an_explicit_timeout_fallback() {
    let service = ScopeCompletionService::default();
    let state = service.0.lock().unwrap();
    let observed = state.epoch;
    assert_eq!(
        service.park_if_unchanged(state, observed, std::time::Duration::ZERO),
        ScopePark::TimedOut
    );
}

#[test]
fn error_variants_expose_underlying_source() {
    use std::error::Error as StdError;
    let oom = Error::OutOfMemory {
        source: Arc::new(std::io::Error::new(
            std::io::ErrorKind::OutOfMemory,
            "backing store full",
        )),
    };
    let val = Error::Validation {
        source: Arc::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bad binding",
        )),
        description: "validation failed".into(),
    };
    let internal = Error::Internal {
        source: Arc::new(std::io::Error::other("driver reset")),
        description: "internal failure".into(),
    };
    assert!(
        oom.source()
            .unwrap()
            .to_string()
            .contains("backing store full")
    );
    assert!(val.source().unwrap().to_string().contains("bad binding"));
    assert!(
        internal
            .source()
            .unwrap()
            .to_string()
            .contains("driver reset")
    );
}
