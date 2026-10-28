//! Temporarily move primary/auxiliary destinations out of the pool so all
//! source borrows remain disjoint; restore both before propagating an error.
use crate::CudaError;

pub(super) fn with_outputs<T, R>(
    slots: &mut [Option<T>],
    primary: usize,
    auxiliary: Option<usize>,
    launch: impl FnOnce(&[Option<T>], &mut T, Option<&mut T>) -> Result<R, CudaError>,
) -> Result<R, CudaError> {
    if slots.get(primary).is_none_or(Option::is_none)
        || auxiliary.is_some_and(|i| i == primary || slots.get(i).is_none_or(Option::is_none))
    {
        return Err(CudaError::InvalidInput(
            "invalid prepared scratch destinations",
        ));
    }
    let main = slots[primary].take();
    let extra = auxiliary.and_then(|i| slots[i].take());
    let mut taken = TakenOutputs {
        slots,
        primary,
        auxiliary,
        main,
        extra,
    };
    launch(
        taken.slots,
        taken.main.as_mut().expect("validated primary destination"),
        taken.extra.as_mut(),
    )
}

/// Restores storage before unwinding reaches an outer CUDA capture guard. No
/// device allocation may be dropped while that stream is still being captured.
struct TakenOutputs<'a, T> {
    slots: &'a mut [Option<T>],
    primary: usize,
    auxiliary: Option<usize>,
    main: Option<T>,
    extra: Option<T>,
}
impl<T> Drop for TakenOutputs<'_, T> {
    fn drop(&mut self) {
        self.slots[self.primary] = self.main.take();
        if let Some(i) = self.auxiliary {
            self.slots[i] = self.extra.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_destinations_are_disjoint_and_restored_after_enqueue_failure() {
        let mut slots = vec![Some(3), Some(4), Some(5)];
        let result: Result<(), _> = with_outputs(&mut slots, 2, Some(0), |sources, main, extra| {
            assert!(sources[0].is_none() && sources[2].is_none());
            assert_eq!(sources[1], Some(4));
            *main = 7;
            *extra.unwrap() = 8;
            Err(CudaError::InvalidInput("mock enqueue failure"))
        });
        assert!(result.is_err());
        assert_eq!(slots, [Some(8), Some(4), Some(7)]);
    }
    #[test]
    fn bad_slots_do_not_take_storage_or_call_launch() {
        let mut slots = vec![Some(3), None];
        for (primary, extra) in [
            (0, Some(0)),
            (0, Some(1)),
            (0, Some(2)),
            (1, None),
            (2, None),
        ] {
            let result: Result<(), _> = with_outputs(&mut slots, primary, extra, |_, _, _| {
                panic!("must not enqueue")
            });
            assert!(result.is_err());
            assert_eq!(slots, [Some(3), None]);
        }
    }
    #[test]
    fn panic_restores_both_destinations_before_outer_capture_cleanup() {
        use std::{cell::RefCell, rc::Rc};
        struct Marker(&'static str, Rc<RefCell<Vec<&'static str>>>);
        impl Drop for Marker {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }
        for auxiliary in [None, Some(2)] {
            let events = Rc::new(RefCell::new(Vec::new()));
            let mut slots = vec![
                Some(Marker("main dropped", events.clone())),
                Some(Marker("source dropped", events.clone())),
                Some(Marker("extra dropped", events.clone())),
            ];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _capture = Marker("capture ended", events.clone());
                let _: Result<(), CudaError> =
                    with_outputs(&mut slots, 0, auxiliary, |sources, main, extra| {
                        assert!(sources[0].is_none());
                        assert_eq!(extra.is_some(), auxiliary.is_some());
                        main.0 = "updated main dropped";
                        if let Some(extra) = extra {
                            extra.0 = "updated extra dropped";
                        }
                        events.borrow_mut().push("work enqueued");
                        panic!("injected launch panic");
                    });
            }));
            assert!(result.is_err());
            assert!(slots.iter().all(Option::is_some));
            assert_eq!(slots[0].as_ref().unwrap().0, "updated main dropped");
            assert_eq!(&*events.borrow(), &["work enqueued", "capture ended"]);
            drop(slots);
            assert_eq!(&events.borrow()[..2], &["work enqueued", "capture ended"]);
            assert_eq!(events.borrow().len(), 5);
        }
    }
}
