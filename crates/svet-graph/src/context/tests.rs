use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn contexts_reject_foreign_equal_numeric_ids() {
    let mut a = Context::new();
    let mut b = Context::new();
    let x = a.create(Kind::Signal).unwrap();
    let y = b.create(Kind::Signal).unwrap();
    assert_eq!(x.id(), y.id());
    assert_ne!(x, y);
    let keys: std::collections::HashSet<_> =
        [x.clone(), x.clone(), y.clone()].into_iter().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(b.notify(&x), Err(Error::ForeignContext));
    assert_eq!(a.dispose(&y), Err(Error::ForeignContext));
    assert_eq!(a.telemetry().nodes, 1);
    assert_eq!(b.telemetry().nodes, 1);
}

#[test]
fn stale_handles_cannot_touch_recycled_or_reset_slots() {
    let mut graph = Context::new();
    let old = graph.create(Kind::Signal).unwrap();
    graph.dispose(&old).unwrap();
    let next = graph.create(Kind::Signal).unwrap();
    assert_eq!(old.id(), next.id());
    assert_eq!(graph.notify(&old), Err(Error::StaleNode));
    graph.reset().unwrap();
    let fresh = graph.create(Kind::Signal).unwrap();
    assert_eq!(next.id(), fresh.id());
    assert_eq!(graph.dispose(&next), Err(Error::StaleNode));
    assert_eq!(graph.telemetry().nodes, 1);
    graph.dispose(&fresh).unwrap();
    assert_eq!(graph.telemetry().nodes, 0);
}

#[test]
fn dynamic_binding_reads_replace_dependencies_and_batches_deduplicate() {
    let mut graph = Context::new();
    let left = graph.create(Kind::Signal).unwrap();
    let right = graph.create(Kind::Signal).unwrap();
    let view = graph.create(Kind::Effect).unwrap();
    graph
        .track(&view, |frame| frame.read(&left).unwrap())
        .unwrap();
    graph
        .batch(|graph| {
            graph.notify(&left).unwrap();
            graph.notify(&left).unwrap();
            assert!(graph.take_effects().is_empty());
        })
        .unwrap();
    let jobs = graph.take_effects();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id(), view.id());
    graph
        .track(&jobs[0], |frame| frame.read(&right).unwrap())
        .unwrap();
    graph.notify(&left).unwrap();
    assert!(graph.take_effects().is_empty());
    graph.notify(&right).unwrap();
    assert_eq!(graph.take_effects().len(), 1);
    for node in [&view, &left, &right] {
        graph.dispose(node).unwrap();
    }
    assert_eq!(graph.telemetry().nodes, 0);
    assert_eq!(graph.telemetry().edges, 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unwind_restores_tracking_and_batch_scopes() {
    let mut graph = Context::new();
    let source = graph.create(Kind::Signal).unwrap();
    let view = graph.create(Kind::Effect).unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| graph.track(&view, |frame| {
            frame.read(&source).unwrap();
            panic!("render");
        })))
        .is_err()
    );
    assert_eq!(graph.telemetry().tracking_depth, 0);
    assert!(
        catch_unwind(AssertUnwindSafe(|| graph.batch(|graph| {
            graph.notify(&source).unwrap();
            assert_eq!(graph.reset(), Err(Error::ScopeActive));
            panic!("batch");
        })))
        .is_err()
    );
    assert_eq!(graph.telemetry().batch_depth, 0);
    assert_eq!(graph.take_effects().len(), 1);
    graph.reset().unwrap();
    assert_eq!(graph.telemetry().nodes, 0);
}

#[test]
fn disposed_queued_effect_does_not_schedule_reused_slot() {
    let mut graph = Context::new();
    let source = graph.create(Kind::Signal).unwrap();
    let old = graph.create(Kind::Effect).unwrap();
    graph
        .track(&old, |frame| frame.read(&source).unwrap())
        .unwrap();
    graph.notify(&source).unwrap();
    graph.dispose(&old).unwrap();
    let next = graph.create(Kind::Effect).unwrap();
    assert_eq!(old.id(), next.id());
    assert!(graph.take_effects().is_empty());
    assert_eq!(graph.track(&old, |_| ()), Err(Error::StaleNode));
    graph
        .track(&next, |frame| frame.read(&source).unwrap())
        .unwrap();
    graph.notify(&source).unwrap();
    assert_eq!(graph.take_effects().len(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn context_drop_expires_weak_handles_and_owned_graph_can_move_threads() {
    let mut graph = Context::new();
    let source = graph.create(Kind::Signal).unwrap();
    let copy = source.clone();
    std::thread::spawn(move || {
        graph.notify(&source).unwrap();
        graph.dispose(&source).unwrap();
    })
    .join()
    .unwrap();
    assert!(!copy.context_alive());
}

#[test]
fn identity_exhaustion_refuses_before_allocating() {
    let mut graph = Context::new();
    graph.next_serial = u64::MAX;
    assert!(matches!(
        graph.create(Kind::Signal),
        Err(Error::IdentityExhausted)
    ));
    assert_eq!(graph.telemetry().nodes, 0);
}

#[test]
fn tracking_rejects_cycles_without_leaving_frames() {
    let mut graph = Context::new();
    let a = graph.create(Kind::Computed).unwrap();
    let b = graph.create(Kind::Computed).unwrap();
    graph.track(&b, |frame| frame.read(&a).unwrap()).unwrap();
    assert_eq!(
        graph.track(&a, |frame| frame.read(&b)),
        Ok(Err(Error::Cycle))
    );
    assert_eq!(
        graph.track(&a, |frame| frame.read(&a)),
        Ok(Err(Error::Cycle))
    );
    assert_eq!(graph.telemetry().tracking_depth, 0);
    let effect = graph.create(Kind::Effect).unwrap();
    assert_eq!(graph.notify(&effect), Err(Error::WrongKind));
}

#[test]
fn native_diamond_evaluation_publishes_one_coherent_result() {
    let mut graph = Context::new();
    let source = graph.create(Kind::Signal).unwrap();
    let left = graph.create(Kind::Computed).unwrap();
    let right = graph.create(Kind::Computed).unwrap();
    let view = graph.create(Kind::Effect).unwrap();
    let render = |graph: &mut Context, value: i32| {
        graph
            .track(&view, |frame| {
                let a = frame
                    .evaluate(&left, |reads| {
                        reads.read(&source).unwrap();
                        value * 2
                    })
                    .unwrap();
                let b = frame
                    .evaluate(&right, |reads| {
                        reads.read(&source).unwrap();
                        value * 3
                    })
                    .unwrap();
                a + b
            })
            .unwrap()
    };
    assert_eq!(render(&mut graph, 1), 5);
    graph
        .batch(|graph| {
            graph.notify(&source).unwrap();
            graph.notify(&source).unwrap();
        })
        .unwrap();
    assert_eq!(graph.take_effects().len(), 1);
    assert_eq!(render(&mut graph, 3), 15);
    assert_eq!(graph.telemetry().edges, 4);
    assert_eq!(graph.telemetry().tracking_depth, 0);
    graph.reset().unwrap();
    assert_eq!(graph.telemetry().nodes, 0);
}

#[test]
fn recursive_computed_evaluation_is_refused_and_outer_frame_restored() {
    let mut graph = Context::new();
    let computed = graph.create(Kind::Computed).unwrap();
    graph
        .track(&computed, |frame| {
            assert_eq!(frame.evaluate(&computed, |_| ()), Err(Error::Cycle));
        })
        .unwrap();
    assert_eq!(graph.telemetry().tracking_depth, 0);
}

#[test]
fn drained_frame_job_cannot_render_a_replacement_lifetime() {
    let mut graph = Context::new();
    let source = graph.create(Kind::Signal).unwrap();
    let view = graph.create(Kind::Effect).unwrap();
    graph
        .track(&view, |reads| reads.read(&source).unwrap())
        .unwrap();
    graph.notify(&source).unwrap();
    let jobs = graph.take_effects();
    graph.dispose(&view).unwrap();
    let replacement = graph.create(Kind::Effect).unwrap();
    assert_eq!(jobs[0].id(), replacement.id());
    let mut rendered = false;
    assert_eq!(
        graph.track(&jobs[0], |_| rendered = true),
        Err(Error::StaleNode)
    );
    assert!(!rendered);
    graph
        .track(&replacement, |reads| reads.read(&source).unwrap())
        .unwrap();
    graph.notify(&source).unwrap();
    let next = graph.take_effects();
    assert_eq!(next, vec![replacement]);
}
