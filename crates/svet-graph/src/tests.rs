use super::*;

#[test]
fn test_diamond_dependency_glitch_free() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_COMPUTED);
    let c = g.create_node(KIND_COMPUTED);
    let d = g.create_node(KIND_EFFECT);

    g.begin_tracking(b);
    g.track_read(a);
    g.end_tracking();

    g.begin_tracking(c);
    g.track_read(a);
    g.end_tracking();

    g.begin_tracking(d);
    g.track_read(b);
    g.track_read(c);
    g.end_tracking();

    g.trigger_write(a);

    assert_eq!(g.get_state(b), STATE_DIRTY);
    assert_eq!(g.get_state(c), STATE_DIRTY);

    let count = g.prepare_flush();
    assert_eq!(count, 1);
    assert_eq!(g.flush_buffer[0], d);
}

#[test]
fn test_free_list_slot_recycling() {
    let mut g = ReactiveGraph::new();
    let n0 = g.create_node(KIND_SIGNAL);
    let n1 = g.create_node(KIND_EFFECT);
    assert_eq!(g.active_count, 2);
    assert_eq!(g.nodes.len(), 2);

    g.dispose_node(n1);
    assert_eq!(g.active_count, 1);
    assert_eq!(g.nodes.len(), 2);

    // Next allocated node MUST reuse slot `n1` without growing `nodes` vector
    let n2 = g.create_node(KIND_EFFECT);
    assert_eq!(n2, n1);
    assert_eq!(g.active_count, 2);
    assert_eq!(g.nodes.len(), 2);
    let _ = n0;
}

#[test]
fn dependency_reachability_handles_diamonds_cycles_and_disposal() {
    let mut g = ReactiveGraph::new();
    let s = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_COMPUTED);
    let b = g.create_node(KIND_COMPUTED);
    let c = g.create_node(KIND_COMPUTED);
    g.link(s, a);
    g.link(s, b);
    g.link(a, c);
    g.link(b, c);
    assert!(g.depends_on(c, s));
    assert!(!g.depends_on(s, c));
    g.link(c, a);
    let independent = g.create_node(KIND_SIGNAL);
    assert!(!g.depends_on(c, independent));
    g.dispose_node(s);
    assert!(!g.depends_on(c, s));
}

#[test]
fn dependency_queries_reuse_scratch_and_leave_reactive_state_unchanged() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.link(source, derived);
    assert!(g.depends_on(derived, source));
    let stack = g.dependency_stack.as_ptr();
    let seen = g.dependency_seen.as_ptr();
    let states = g.node_states.clone();
    let topology = g.topology_epoch;
    let clock = g.global_epoch;
    for _ in 0..100 {
        assert!(g.depends_on(derived, source));
    }
    assert_eq!(g.dependency_stack.as_ptr(), stack);
    assert_eq!(g.dependency_seen.as_ptr(), seen);
    assert_eq!(g.node_states, states);
    assert_eq!(g.topology_epoch, topology);
    assert_eq!(g.global_epoch, clock);
    g.dependency_epoch = u32::MAX;
    assert!(g.depends_on(derived, source));
    assert_eq!(g.node_states, states);
}

#[test]
fn pending_queue_does_not_run_a_replacement_that_was_never_scheduled() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.sync_sources(old, &[source]);
    g.trigger_write(source);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_queue_preserves_real_schedule_order_when_a_slot_is_reused() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    let peer = g.create_node(KIND_EFFECT);
    g.sync_sources(old, &[a]);
    g.sync_sources(peer, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.sync_sources(fresh, &[c]);
    g.trigger_write(c);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![peer, fresh]);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_queue_rejects_replacement_signal_and_computed_slots() {
    for kind in [KIND_SIGNAL, KIND_COMPUTED] {
        let mut g = ReactiveGraph::new();
        let source = g.create_node(KIND_SIGNAL);
        let old = g.create_node(KIND_EFFECT);
        g.sync_sources(old, &[source]);
        g.trigger_write(source);
        g.dispose_node(old);
        assert_eq!(g.create_node(kind), old);
        assert_eq!(g.prepare_flush(), 0);
    }
}

#[test]
fn pending_lifetime_wrap_purges_old_tokens_without_dropping_other_jobs() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    let peer = g.create_node(KIND_EFFECT);
    g.pending_effects.push(ScheduledEffect {
        id: old,
        lifetime: 1,
    });
    g.nodes[old as usize].lifetime = u32::MAX;
    g.sync_sources(old, &[a]);
    g.sync_sources(peer, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.nodes[fresh as usize].lifetime, 1);
    assert!(g.pending_effects.iter().all(|pending| pending.id != old));
    assert_eq!(g.pending_effect_count(), 1);
    g.sync_sources(fresh, &[a]);
    g.trigger_write(a);
    assert_eq!(g.pending_effect_count(), 2);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![peer, fresh]);
}

#[test]
fn pending_count_observes_dirty_work_without_consuming_it() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.pending_effect_count(), 1);
    assert!(g.nodes[effect as usize].in_queue);
    assert_eq!(g.pending_effects.len(), 1);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.pending_effect_count(), 0);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.pending_effects.len(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.pending_effect_count(), 0);
}

#[test]
fn pending_churn_keeps_entry_count_and_capacity_bounded_without_flushing() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let mut warm_capacity = 0;
    for i in 0..10_000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[source]);
        g.trigger_write(source);
        g.dispose_node(effect);
        assert!(
            g.pending_effects.len() <= 66,
            "expired pending jobs grew with churn"
        );
        if i == 200 {
            warm_capacity = g.pending_effects.capacity();
        }
    }
    assert_eq!(g.active_count, 1);
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.pending_effects.capacity(), warm_capacity);
    assert_eq!(g.pending_effect_count(), 0);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_compaction_preserves_live_job_order_and_flags() {
    let mut g = ReactiveGraph::new();
    let churn = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let left = g.create_node(KIND_EFFECT);
    let right = g.create_node(KIND_EFFECT);
    g.sync_sources(left, &[a]);
    g.sync_sources(right, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    for _ in 0..1000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[churn]);
        g.trigger_write(churn);
        g.dispose_node(effect);
    }
    assert!(g.pending_effects.len() <= 74);
    assert!(g.nodes[left as usize].in_queue && g.nodes[right as usize].in_queue);
    assert_eq!(g.pending_effect_count(), 2);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![left, right]);
}

#[test]
fn pending_compaction_retains_clean_scheduled_jobs_for_later_writes() {
    let mut g = ReactiveGraph::new();
    let churn = g.create_node(KIND_SIGNAL);
    let source = g.create_node(KIND_SIGNAL);
    let live = g.create_node(KIND_EFFECT);
    g.sync_sources(live, &[source]);
    g.trigger_write(source);
    g.set_state(live, STATE_CLEAN);
    for _ in 0..1000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[churn]);
        g.trigger_write(churn);
        g.dispose_node(effect);
    }
    assert!(g.pending_effects.len() <= 70);
    assert_eq!(g.pending_effect_count(), 0);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.flush_buffer, vec![live]);
}

#[test]
fn pending_population_drop_compacts_expired_entries_without_new_writes() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effects: Vec<_> = (0..1000)
        .map(|_| {
            let id = g.create_node(KIND_EFFECT);
            g.sync_sources(id, &[source]);
            id
        })
        .collect();
    g.trigger_write(source);
    for &id in &effects[1..] {
        g.dispose_node(id);
    }
    assert!(g.pending_effects.len() <= 68);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.flush_buffer, vec![effects[0]]);
}

#[test]
fn cached_no_effect_source_still_invalidates_lazy_computeds() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.sync_sources(derived, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    g.set_state(derived, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(derived), STATE_DIRTY);
}

#[test]
fn cached_terminal_source_does_not_dirty_unrelated_computeds() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let own = g.create_node(KIND_COMPUTED);
    let unrelated = g.create_node(KIND_COMPUTED);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(own, &[source]);
    g.sync_sources(unrelated, &[other]);
    g.sync_sources(effect, &[own]);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(own, STATE_CLEAN);
    g.set_state(unrelated, STATE_CLEAN);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.get_state(unrelated), STATE_CLEAN);
}

#[test]
fn cached_empty_source_drains_preexisting_foreign_pending_effects() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[other]);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.pending_effect_count(), 0);
}

#[test]
fn terminal_cache_is_source_specific_when_a_foreign_effect_is_drained() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    let derived = g.create_node(KIND_COMPUTED);
    g.sync_sources(effect, &[other]);
    g.sync_sources(derived, &[source]);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    g.set_state(derived, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(effect), STATE_CLEAN);
    assert_eq!(g.get_state(derived), STATE_DIRTY);
}

#[test]
fn topology_wrap_does_not_restore_a_stale_no_subscriber_cache() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    let effect = g.create_node(KIND_EFFECT);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.sync_sources(effect, &[source]);
    g.link(other, derived);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
}

#[test]
fn topology_wrap_does_not_restore_a_detached_effect_cache() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    let other = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_COMPUTED);
    let b = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.clear_sources(effect);
    g.link(other, a);
    g.link(other, b);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
}

#[test]
fn topology_wrap_cannot_dirty_a_signal_reusing_a_cached_effect_slot() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.dispose_node(effect);
    let replacement = g.create_node(KIND_SIGNAL);
    assert_eq!(replacement, effect);
    g.link(other, derived);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(replacement), STATE_CLEAN);
}

#[test]
fn topology_wrap_invalidates_cached_downstream_computed_slots() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(derived, &[source]);
    g.sync_sources(effect, &[derived]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.downstream_computeds[source as usize], vec![derived]);
    g.topology_epoch = u32::MAX;
    g.clear_sources(derived);
    g.clear_sources(effect);
    g.dispose_node(derived);
    g.dispose_node(effect);
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    assert_eq!(a, effect);
    assert_eq!(b, derived);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(a), STATE_CLEAN);
    assert_eq!(g.get_state(b), STATE_CLEAN);
}

#[test]
fn topology_wrap_rebuilds_cache_and_noop_links_preserve_it() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.link(other, derived);
    assert_eq!(g.topology_epoch, 1);
    assert_eq!(g.terminal_cache_epoch[source as usize], 0);
    assert_eq!(g.single_terminal_effect[source as usize], u32::MAX);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.terminal_cache_epoch[source as usize], g.topology_epoch);
    let epoch = g.topology_epoch;
    g.link(source, effect);
    g.clear_sources(other);
    assert_eq!(g.topology_epoch, epoch);
    assert_eq!(g.terminal_cache_epoch[source as usize], epoch);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.get_state(effect), STATE_DIRTY);
}

#[test]
fn cursor_pruning_invalidates_cached_terminal_effects() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    g.begin_tracking(e);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(b), 0);
}

#[test]
fn cursor_replacement_invalidates_both_old_and_new_source_caches() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a]);
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(b), 0);
    g.begin_tracking(e);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(a), 0);
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
}

#[test]
fn cursor_dependency_reordering_preserves_both_forward_edges() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    g.begin_tracking(e);
    g.track_read(b);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![b, a]);
    assert!(g.subscribers[a as usize].contains(&e));
    assert!(g.subscribers[b as usize].contains(&e));
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
}

#[test]
fn cursor_ignores_self_and_invalid_reads_without_advancing() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(e);
    g.track_read(u32::MAX);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a]);
}

#[test]
fn cursor_cannot_mutate_a_replacement_observer_lifetime() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.track_read(a);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.track_read(b);
    g.end_tracking();
    assert!(g.sources[fresh as usize].is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_stale_inner_frame_preserves_the_outer_tracker() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let outer = g.create_node(KIND_EFFECT);
    let inner = g.create_node(KIND_COMPUTED);
    g.begin_tracking(outer);
    g.track_read(a);
    g.begin_tracking(inner);
    g.track_read(a);
    g.dispose_node(inner);
    let fresh = g.create_node(KIND_COMPUTED);
    assert_eq!(fresh, inner);
    g.track_read(b);
    g.end_tracking();
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.sources[outer as usize], vec![a, b]);
    assert!(g.sources[fresh as usize].is_empty());
    assert!(g.tracking_stack.is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_lifetime_wrap_invalidates_ancient_tracking_frames() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.nodes[old as usize].lifetime = u32::MAX;
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.nodes[fresh as usize].lifetime, 1);
    g.track_read(a);
    g.end_tracking();
    assert!(g.sources[fresh as usize].is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_source_disposal_does_not_turn_an_unread_tail_into_a_read_prefix() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    g.begin_tracking(e);
    g.track_read(a);
    g.dispose_node(a);
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![c]);
    assert!(!g.subscribers[b as usize].contains(&e));
}

#[test]
fn cursor_explicit_source_replacement_restarts_the_discovery_cursor() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let d = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.sync_sources(e, &[c, d]);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![b]);
}

#[test]
fn cursor_pause_and_invalid_begin_preserve_outer_tracking() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.pause_tracking();
    g.track_read(b);
    g.resume_tracking();
    g.begin_tracking(u32::MAX);
    g.track_read(b);
    g.end_tracking();
    g.begin_tracking(a);
    g.track_read(b);
    g.end_tracking();
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a, c]);
    assert!(!g.subscribers[b as usize].contains(&e));
    assert!(g.tracking_stack.is_empty());
}

#[test]
fn cursor_stable_and_reordered_reads_reuse_storage_and_topology_cache() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    let sources = g.sources[e as usize].as_ptr();
    let subs_a = g.subscribers[a as usize].as_ptr();
    let subs_b = g.subscribers[b as usize].as_ptr();
    let stack = g.tracking_stack.as_ptr();
    let epoch = g.topology_epoch;
    for i in 0..100 {
        g.begin_tracking(e);
        if i % 2 == 0 {
            g.track_read(b);
            g.track_read(a);
        } else {
            g.track_read(a);
            g.track_read(b);
        }
        g.end_tracking();
    }
    assert_eq!(g.sources[e as usize].as_ptr(), sources);
    assert_eq!(g.subscribers[a as usize].as_ptr(), subs_a);
    assert_eq!(g.subscribers[b as usize].as_ptr(), subs_b);
    assert_eq!(g.tracking_stack.as_ptr(), stack);
    assert_eq!(g.topology_epoch, epoch);
    assert_eq!(g.terminal_cache_epoch[a as usize], epoch);
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
}

#[test]
fn cursor_recursive_begin_does_not_replace_the_outer_dependency_pass() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.begin_tracking(e);
    g.track_read(b);
    g.end_tracking();
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a, c]);
    assert!(!g.subscribers[b as usize].contains(&e));
    assert!(g.tracking_stack.is_empty());
}

#[test]
fn cursor_fresh_lifetime_can_track_before_the_stale_frame_is_closed() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.track_read(a);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.begin_tracking(fresh);
    g.track_read(b);
    g.end_tracking();
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[fresh as usize], vec![b]);
    assert_eq!(g.get_state(fresh), STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(a), 0);
    assert_eq!(g.trigger_and_flush_fast(b), (fresh as i32) + 1);
}

#[test]
fn long_subscriber_lists_stay_consistent_through_link_unlink_and_reuse() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let effects: Vec<u32> = (0..200).map(|_| g.create_node(KIND_EFFECT)).collect();
    for &effect in &effects {
        g.sync_sources(effect, &[source, other, source]);
    }
    assert!(g.sub_indexed[source as usize]);
    assert_eq!(g.subscribers[source as usize].len(), 200);
    assert_eq!(g.sub_index.len(), 400);
    let check = |g: &ReactiveGraph| {
        for node in [source, other] {
            for (position, &sub) in g.subscribers[node as usize].iter().enumerate() {
                assert_eq!(g.sub_index[&edge_key(node as usize, sub)], position as u32);
                assert!(g.sources[sub as usize].contains(&node));
            }
        }
    };
    check(&g);
    // Unlinking from the middle moves the last subscriber into the hole.
    for &effect in effects.iter().step_by(3) {
        g.sync_sources(effect, &[other]);
    }
    check(&g);
    assert_eq!(g.subscribers[source as usize].len(), 133);
    for &effect in effects.iter().skip(1).step_by(3) {
        g.dispose_node(effect);
    }
    check(&g);
    assert_eq!(g.subscribers[source as usize].len(), 66);
    assert_eq!(g.subscribers[other as usize].len(), 133);
    // Disposing the source drops its index entries; a reused slot starts clean.
    g.dispose_node(source);
    assert_eq!(g.sub_index.len(), 133);
    let reused = g.create_node(KIND_SIGNAL);
    assert_eq!(reused, source);
    assert!(!g.sub_indexed[reused as usize] && g.subscribers[reused as usize].is_empty());
    for &effect in effects.iter().skip(2).step_by(3) {
        assert_eq!(g.sources[effect as usize], vec![other]);
    }
    g.reset();
    assert!(g.sub_index.is_empty() && g.sub_indexed.is_empty());
}
