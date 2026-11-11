use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

pub const KIND_SIGNAL: u8 = 0;
pub const KIND_COMPUTED: u8 = 1;
pub const KIND_EFFECT: u8 = 2;

pub const STATE_CLEAN: u8 = 0;
pub const STATE_CHECK: u8 = 1;
pub const STATE_DIRTY: u8 = 2;
pub const STATE_DISPOSED: u8 = 255;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub kind: u8,
    pub in_queue: bool,
    pub tracking: bool,
    /// Identity of this slot allocation, independent of reactive write epochs.
    pub lifetime: u32,
    pub alive: bool,
    pub epoch: u32,
}

impl Node {
    #[inline]
    pub const fn is_observer(&self) -> bool {
        self.kind == KIND_COMPUTED || self.kind == KIND_EFFECT
    }

    #[inline]
    pub const fn matches_observer(&self, lifetime: u32) -> bool {
        self.alive && self.lifetime == lifetime && self.is_observer()
    }

    #[inline]
    pub const fn matches_queued_effect(&self, lifetime: u32) -> bool {
        self.alive && self.kind == KIND_EFFECT && self.in_queue && self.lifetime == lifetime
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScheduledEffect {
    pub id: u32,
    pub lifetime: u32,
}

/// Subscriber lists at least this long keep their positions in `sub_index`.
const SUB_INDEX_THRESHOLD: usize = 16;

/// Hashes one `u64` edge key; the default SipHash costs more than the scan it replaces.
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeHashBuilder;

#[derive(Debug)]
pub struct EdgeHasher(u64);

impl BuildHasher for EdgeHashBuilder {
    type Hasher = EdgeHasher;
    fn build_hasher(&self) -> EdgeHasher {
        EdgeHasher(0)
    }
}

impl Hasher for EdgeHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }
    fn write_u64(&mut self, value: u64) {
        let mixed = (self.0.rotate_left(5) ^ value).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = mixed ^ (mixed >> 32);
    }
}

#[inline]
pub const fn edge_key(source: usize, target: u32) -> u64 {
    ((source as u64) << 32) | target as u64
}

#[derive(Debug)]
pub struct ReactiveGraph {
    pub nodes: Vec<Node>,
    /// Forward edges: `subscribers[source_id]` -> list of target node IDs
    pub subscribers: Vec<Vec<u32>>,
    /// Backward edges: `sources[target_id]` -> list of source node IDs
    pub sources: Vec<Vec<u32>>,
    /// Position of `target` in `subscribers[source]`, kept only for sources
    /// marked in `sub_indexed`: a signal with thousands of subscribers must
    /// not scan its list on every link and unlink.
    pub sub_index: HashMap<u64, u32, EdgeHashBuilder>,
    pub sub_indexed: Vec<bool>,
    /// Contiguous byte array of node states (`STATE_CLEAN`, `STATE_CHECK`, `STATE_DIRTY`, `STATE_DISPOSED`)
    /// exposed directly to JS via zero-copy `Uint8Array` view so JS can check `state === STATE_CLEAN` without FFI.
    pub node_states: Vec<u8>,
    /// Cached terminal effect (`u32::MAX` = multiple/dirty, `u32::MAX - 1` = none, `< u32::MAX - 1` = unique downstream effect ID)
    pub single_terminal_effect: Vec<u32>,
    /// Cached downstream computed nodes for `source_id` so `trigger_and_flush_fast` can mark them in a flat slice loop
    pub downstream_computeds: Vec<Vec<u32>>,
    /// Epoch of the topology structure (incremented whenever `link`, `clear_sources`, or `dispose_node` mutates edges)
    pub topology_epoch: u32,
    /// Epoch when `single_terminal_effect[source]` was last validated
    pub terminal_cache_epoch: Vec<u32>,
    /// Recycled node IDs for O(1) slot reuse without memory growth
    pub free_list: Vec<u32>,
    /// Count of currently active (non-disposed) nodes
    pub active_count: u32,
    /// Discovery frames `(observer, cursor, slot_lifetime)` for computed/effect evaluation
    pub tracking_stack: Vec<(u32, usize, u32)>,
    /// Scheduled effect identities; native IDs alone can be reused before flush
    pub pending_effects: Vec<ScheduledEffect>,
    /// Output buffer exposed to JS via zero-copy `Uint32Array` view
    pub flush_buffer: Vec<u32>,
    /// Batch depth counter (allows nested `batch(() => ...)` without intermediate flushes)
    pub batch_depth: u32,
    /// Global monotonic version clock
    pub global_epoch: u32,
    /// Reusable traversal stack for `trigger_write` (avoids per-write Vec allocation)
    pub propagate_stack: Vec<(u32, u8)>,
    pub dependency_stack: Vec<u32>,
    pub dependency_seen: Vec<u32>,
    pub dependency_epoch: u32,
}

impl Default for ReactiveGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactiveGraph {
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            subscribers: Vec::new(),
            sources: Vec::new(),
            sub_index: HashMap::with_hasher(EdgeHashBuilder),
            sub_indexed: Vec::new(),
            node_states: Vec::new(),
            single_terminal_effect: Vec::new(),
            downstream_computeds: Vec::new(),
            topology_epoch: 1,
            terminal_cache_epoch: Vec::new(),
            free_list: Vec::new(),
            active_count: 0,
            tracking_stack: Vec::new(),
            pending_effects: Vec::new(),
            flush_buffer: Vec::new(),
            batch_depth: 0,
            global_epoch: 1,
            propagate_stack: Vec::new(),
            dependency_stack: Vec::new(),
            dependency_seen: Vec::new(),
            dependency_epoch: 0,
        }
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
        self.subscribers.clear();
        self.sources.clear();
        self.sub_index.clear();
        self.sub_indexed.clear();
        self.node_states.clear();
        self.single_terminal_effect.clear();
        self.downstream_computeds.clear();
        self.topology_epoch = 1;
        self.terminal_cache_epoch.clear();
        self.free_list.clear();
        self.active_count = 0;
        self.tracking_stack.clear();
        self.pending_effects.clear();
        self.flush_buffer.clear();
        self.batch_depth = 0;
        self.global_epoch = 1;
        self.propagate_stack.clear();
        self.dependency_stack.clear();
        self.dependency_seen.clear();
        self.dependency_epoch = 0;
    }

    #[inline]
    pub fn is_alive(&self, id: u32) -> bool {
        self.nodes.get(id as usize).is_some_and(|node| node.alive)
    }

    #[inline]
    pub fn is_live_observer(&self, id: u32, lifetime: u32) -> bool {
        self.nodes
            .get(id as usize)
            .is_some_and(|node| node.matches_observer(lifetime))
    }

    #[inline]
    pub fn is_tracking_active(&self) -> bool {
        self.tracking_stack
            .last()
            .is_some_and(|&(target, _, lifetime)| self.is_live_observer(target, lifetime))
    }

    #[inline]
    pub fn edge_count(&self) -> usize {
        self.subscribers.iter().map(Vec::len).sum()
    }

    #[inline]
    fn begin_dependency_pass(&mut self) -> u32 {
        self.dependency_seen.resize(self.nodes.len(), 0);
        self.dependency_epoch = self.dependency_epoch.wrapping_add(1);
        if self.dependency_epoch == 0 {
            self.dependency_seen.fill(0);
            self.dependency_epoch = 1;
        }
        self.dependency_stack.clear();
        self.dependency_epoch
    }

    #[inline]
    pub fn create_node(&mut self, kind: u8) -> u32 {
        let initial_state = if kind == KIND_SIGNAL {
            STATE_CLEAN
        } else {
            STATE_DIRTY
        };
        self.active_count += 1;

        if let Some(reused_id) = self.free_list.pop() {
            let idx = reused_id as usize;
            let mut lifetime = self.nodes[idx].lifetime.wrapping_add(1);
            if lifetime == 0 {
                // Avoid aliasing any ancient token when a slot's counter wraps.
                self.pending_effects
                    .retain(|pending| pending.id != reused_id);
                for frame in &mut self.tracking_stack {
                    if frame.0 == reused_id {
                        *frame = (u32::MAX, 0, 0);
                    }
                }
                lifetime = 1;
            }
            self.nodes[idx] = Node {
                lifetime,
                kind,
                in_queue: false,
                tracking: false,
                alive: true,
                epoch: self.global_epoch,
            };
            self.node_states[idx] = initial_state;
            self.single_terminal_effect[idx] = u32::MAX;
            self.downstream_computeds[idx].clear();
            self.terminal_cache_epoch[idx] = 0;
            self.take_subscribers(idx);
            self.sources[idx].clear();
            return reused_id;
        }

        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            lifetime: 1,
            kind,
            in_queue: false,
            tracking: false,
            alive: true,
            epoch: self.global_epoch,
        });
        self.node_states.push(initial_state);
        self.single_terminal_effect.push(u32::MAX);
        self.downstream_computeds.push(Vec::new());
        self.terminal_cache_epoch.push(0);
        self.subscribers.push(Vec::with_capacity(4));
        self.sub_indexed.push(false);
        self.sources.push(Vec::with_capacity(4));
        id
    }

    #[inline]
    fn advance_topology_epoch(&mut self) {
        self.topology_epoch = self.topology_epoch.wrapping_add(1);
        if self.topology_epoch == 0 {
            self.clear_wrapped_topology_caches();
        }
    }

    #[cold]
    fn clear_wrapped_topology_caches(&mut self) {
        // Zero remains an invalid cache stamp; no stamp from an older cycle may match.
        self.topology_epoch = 1;
        self.terminal_cache_epoch.fill(0);
        self.single_terminal_effect.fill(u32::MAX);
        for cache in &mut self.downstream_computeds {
            cache.clear();
        }
    }

    /// Disposes a node completely, severing both incoming and outgoing edges
    /// and returning its ID to `free_list` for O(1) slot recycling.
    #[inline]
    pub fn dispose_node(&mut self, id: u32) {
        let idx = id as usize;
        if !self.is_alive(id) {
            return;
        }
        self.advance_topology_epoch();

        // 1. Sever incoming edges (sources -> this node)
        self.clear_sources(id);

        // 2. Sever outgoing edges (this node -> subscribers)
        let subs = self.take_subscribers(idx);
        for sub_id in subs {
            let lifetime = self
                .nodes
                .get(sub_id as usize)
                .filter(|node| node.alive && node.tracking)
                .map(|node| node.lifetime);
            if let Some(sub_sources) = self.sources.get_mut(sub_id as usize)
                && let Some(pos) = sub_sources.iter().position(|&x| x == id)
            {
                let mut tracked = false;
                for frame in &mut self.tracking_stack {
                    if frame.0 == sub_id && Some(frame.2) == lifetime {
                        tracked = true;
                        if pos < frame.1 {
                            frame.1 -= 1;
                        }
                    }
                }
                // Preserve the already-read prefix of active discovery passes.
                if tracked {
                    sub_sources.remove(pos);
                } else {
                    sub_sources.swap_remove(pos);
                }
            }
        }

        // 3. Mark dead and push to free_list
        let node = &mut self.nodes[idx];
        node.alive = false;
        node.in_queue = false;
        node.tracking = false;
        self.node_states[idx] = STATE_DISPOSED;
        if self.active_count > 0 {
            self.active_count -= 1;
        }
        self.free_list.push(id);
        self.compact_pending_effects_if_needed();
    }

    #[inline]
    pub fn link(&mut self, source: u32, target: u32) {
        let s = source as usize;
        let t = target as usize;
        if s == t || !self.is_alive(source) || !self.is_alive(target) {
            return;
        }
        // Both lists hold every edge, so one membership test covers the pair.
        if !self.has_subscriber(s, target) {
            debug_assert!(!self.sources[t].contains(&source));
            self.add_subscriber(s, target);
            self.sources[t].push(source);
            self.advance_topology_epoch();
        }
    }

    #[inline]
    fn has_subscriber(&self, source: usize, target: u32) -> bool {
        if self.sub_indexed[source] {
            self.sub_index.contains_key(&edge_key(source, target))
        } else {
            self.subscribers[source].contains(&target)
        }
    }

    /// The caller has checked that the edge is absent.
    #[inline]
    fn add_subscriber(&mut self, source: usize, target: u32) {
        let subs = &mut self.subscribers[source];
        subs.push(target);
        if self.sub_indexed[source] {
            self.sub_index
                .insert(edge_key(source, target), subs.len() as u32 - 1);
        } else if subs.len() >= SUB_INDEX_THRESHOLD {
            self.sub_indexed[source] = true;
            for (position, &sub) in subs.iter().enumerate() {
                self.sub_index
                    .insert(edge_key(source, sub), position as u32);
            }
        }
    }

    /// Removes the edge if present; the list's last entry takes its place.
    #[inline]
    fn remove_subscriber(&mut self, source: usize, target: u32) {
        let Some(subs) = self.subscribers.get_mut(source) else {
            return;
        };
        if !self.sub_indexed[source] {
            if let Some(position) = subs.iter().position(|&id| id == target) {
                subs.swap_remove(position);
            }
            return;
        }
        let Some(position) = self.sub_index.remove(&edge_key(source, target)) else {
            return;
        };
        subs.swap_remove(position as usize);
        if let Some(&moved) = subs.get(position as usize) {
            self.sub_index.insert(edge_key(source, moved), position);
        }
    }

    /// Empties a subscriber list and drops its index entries.
    fn take_subscribers(&mut self, source: usize) -> Vec<u32> {
        let subs = std::mem::take(&mut self.subscribers[source]);
        if self.sub_indexed[source] {
            self.sub_indexed[source] = false;
            for &sub in &subs {
                self.sub_index.remove(&edge_key(source, sub));
            }
        }
        subs
    }

    #[inline]
    pub fn clear_sources(&mut self, target: u32) {
        let t = target as usize;
        if t >= self.nodes.len() {
            return;
        }
        if let Some(node) = self.nodes.get(t).filter(|node| node.alive) {
            for frame in &mut self.tracking_stack {
                if frame.0 == target && frame.2 == node.lifetime {
                    frame.1 = 0;
                }
            }
        }
        if !self.sources[t].is_empty() {
            self.advance_topology_epoch();
        }
        let prev_sources = std::mem::take(&mut self.sources[t]);
        for src in prev_sources {
            self.remove_subscriber(src as usize, target);
        }
    }

    /// Replaces all incoming dependency edges of `target` with `new_sources` and marks `target` as `STATE_CLEAN`.
    pub fn sync_sources(&mut self, target: u32, new_sources: &[u32]) {
        self.clear_sources(target);
        if !self.is_alive(target) {
            return;
        }
        for &src in new_sources {
            self.link(src, target);
        }
        self.node_states[target as usize] = STATE_CLEAN;
    }

    /// Uses cursor-based edge verification: if dependencies are accessed in the same order,
    /// zero vector mutations or allocations occur.
    #[inline]
    pub fn begin_tracking(&mut self, target: u32) {
        if let Some(node) = self.nodes.get_mut(target as usize)
            && node.alive
            && !node.tracking
            && node.is_observer()
        {
            node.tracking = true;
            self.tracking_stack.push((target, 0, node.lifetime));
            return;
        }
        // Preserve stack balance without attaching reads to an outer observer.
        self.tracking_stack.push((u32::MAX, 0, 0));
    }

    #[inline]
    pub fn pause_tracking(&mut self) {
        self.tracking_stack.push((u32::MAX, 0, 0));
    }

    #[inline]
    pub fn resume_tracking(&mut self) {
        if self
            .tracking_stack
            .last()
            .is_some_and(|frame| frame.0 == u32::MAX)
        {
            self.tracking_stack.pop();
        }
    }

    /// Finish only the lifetime that began this frame, pruning unused trailing sources.
    #[inline]
    pub fn end_tracking(&mut self) {
        let Some((target, cursor, lifetime)) = self.tracking_stack.pop() else {
            return;
        };
        if !self.is_live_observer(target, lifetime) {
            return;
        }
        let t = target as usize;
        self.nodes[t].tracking = false;
        let old_len = self.sources[t].len();
        if cursor < old_len {
            self.advance_topology_epoch();
            for i in cursor..old_len {
                let stale_src = self.sources[t][i] as usize;
                self.remove_subscriber(stale_src, target);
            }
            self.sources[t].truncate(cursor);
        }
        self.node_states[t] = STATE_CLEAN;
    }

    /// Verify reads in place; reordering existing sources changes no graph edges.
    #[inline]
    pub fn track_read(&mut self, source: u32) {
        if !self.is_alive(source) {
            return;
        }
        let Some(&(observer, cursor, lifetime)) = self.tracking_stack.last() else {
            return;
        };
        if source == observer || !self.is_live_observer(observer, lifetime) {
            return;
        }
        let t = observer as usize;
        let cur = cursor.min(self.sources[t].len());
        if cur < self.sources[t].len() && self.sources[t][cur] == source {
            self.tracking_stack.last_mut().unwrap().1 = cur + 1;
            return;
        }
        // An edge missing from the source's subscribers is in neither half of
        // the observer's list, so new dependencies skip both scans.
        if self.has_subscriber(source as usize, observer) {
            if self.sources[t][..cur].contains(&source) {
                return;
            }
            if let Some(offset) = self.sources[t][cur..].iter().position(|&id| id == source) {
                self.sources[t].swap(cur, cur + offset);
                self.tracking_stack.last_mut().unwrap().1 = cur + 1;
                return;
            }
        }
        if cur < self.sources[t].len() {
            let old = self.sources[t][cur] as usize;
            self.remove_subscriber(old, observer);
            self.sources[t][cur] = source;
            if !self.has_subscriber(source as usize, observer) {
                self.add_subscriber(source as usize, observer);
            }
            self.advance_topology_epoch();
        } else {
            self.link(source, observer);
        }
        self.tracking_stack.last_mut().unwrap().1 = cur + 1;
    }

    /// Cache only the actual reachable topology, independent of the dirty queue.
    #[inline]
    fn cache_terminal_topology(&mut self, source: usize) {
        self.downstream_computeds[source].clear();
        let mut terminal = u32::MAX - 1;
        if !self.subscribers[source].is_empty() {
            let epoch = self.begin_dependency_pass();
            self.dependency_stack
                .extend(self.subscribers[source].iter().copied());
            while let Some(id) = self.dependency_stack.pop() {
                let i = id as usize;
                if !self.nodes[i].alive || self.dependency_seen[i] == epoch {
                    continue;
                }
                self.dependency_seen[i] = epoch;
                if self.nodes[i].kind == KIND_COMPUTED {
                    self.downstream_computeds[source].push(id);
                    self.dependency_stack
                        .extend(self.subscribers[i].iter().copied());
                } else if self.nodes[i].kind == KIND_EFFECT {
                    terminal = if terminal == u32::MAX - 1 {
                        id
                    } else {
                        u32::MAX
                    };
                }
            }
        }
        self.single_terminal_effect[source] = terminal;
        self.terminal_cache_epoch[source] = self.topology_epoch;
    }

    #[inline]
    fn mark_cached_downstream_dirty(&mut self, source: usize) {
        self.global_epoch = self.global_epoch.wrapping_add(1);
        self.nodes[source].epoch = self.global_epoch;
        for &comp_id in &self.downstream_computeds[source] {
            self.node_states[comp_id as usize] = STATE_DIRTY;
        }
    }

    /// Trigger a change on `source` (signal or computed whose value changed)
    /// and immediately prepare `flush_buffer` if `batch_depth == 0`.
    /// Returns:
    /// - `0` if 0 effects were flushed
    /// - `effect_id + 1` (positive) if exactly 1 effect was flushed
    /// - `-(count as i32)` (negative) if 2 or more effects were flushed
    pub fn trigger_and_flush_fast(&mut self, source: u32) -> i32 {
        if !self.is_alive(source) {
            return 0;
        }
        let s = source as usize;
        // Fast path: if `batch_depth == 0` and this signal has a validated unique downstream effect,
        // mark its cached downstream computeds in a single flat loop without graph stack traversal.
        if self.batch_depth == 0
            && self.pending_effects.is_empty()
            && self.terminal_cache_epoch[s] == self.topology_epoch
        {
            let cached = self.single_terminal_effect[s];
            if cached == u32::MAX - 1 {
                self.mark_cached_downstream_dirty(s);
                return 0;
            }
            if cached < u32::MAX - 1 {
                self.mark_cached_downstream_dirty(s);
                self.node_states[cached as usize] = STATE_DIRTY;
                return (cached as i32) + 1;
            }
        }

        self.trigger_write(source);
        let count = self.prepare_flush();
        if self.batch_depth == 0 {
            self.cache_terminal_topology(s);
        }
        match count {
            0 => 0,
            1 => (self.flush_buffer[0] as i32) + 1,
            _ => -(count as i32),
        }
    }

    /// Propagates `DIRTY` to direct children and `CHECK` to transitive dependents.
    pub fn trigger_write(&mut self, source: u32) {
        if !self.is_alive(source) {
            return;
        }
        let s = source as usize;
        self.global_epoch = self.global_epoch.wrapping_add(1);
        self.nodes[s].epoch = self.global_epoch;

        self.propagate_stack.clear();
        for &sub in &self.subscribers[s] {
            self.propagate_stack.push((sub, STATE_DIRTY));
        }

        while let Some((curr_id, new_state)) = self.propagate_stack.pop() {
            let idx = curr_id as usize;
            let node = &mut self.nodes[idx];
            if !node.alive {
                continue;
            }

            let cur_state = self.node_states[idx];
            if cur_state < new_state {
                self.node_states[idx] = new_state;
            } else {
                continue;
            }

            if node.kind == KIND_EFFECT {
                if !node.in_queue {
                    node.in_queue = true;
                    self.pending_effects.push(ScheduledEffect {
                        id: curr_id,
                        lifetime: node.lifetime,
                    });
                }
            } else if node.kind == KIND_COMPUTED {
                for &sub in &self.subscribers[idx] {
                    self.propagate_stack.push((sub, STATE_CHECK));
                }
            }
        }
        self.compact_pending_effects_if_needed();
    }

    #[inline]
    pub fn get_state(&self, id: u32) -> u8 {
        self.node_states
            .get(id as usize)
            .copied()
            .unwrap_or(STATE_DISPOSED)
    }

    #[inline]
    pub fn set_state(&mut self, id: u32, state: u8) {
        if self.is_alive(id) {
            self.node_states[id as usize] = state;
        }
    }

    pub fn depends_on(&mut self, target: u32, source: u32) -> bool {
        if !self.is_alive(target) || !self.is_alive(source) {
            return false;
        }
        if target == source {
            return true;
        }
        if self.subscribers[source as usize].is_empty() {
            return false;
        }
        let epoch = self.begin_dependency_pass();
        self.dependency_stack.push(target);
        let mut found = false;
        while let Some(id) = self.dependency_stack.pop() {
            if id == source {
                found = true;
                break;
            }
            let i = id as usize;
            if self.dependency_seen[i] == epoch {
                continue;
            }
            self.dependency_seen[i] = epoch;
            self.dependency_stack
                .extend(self.sources[i].iter().copied());
        }
        self.dependency_stack.clear();
        found
    }

    fn compact_pending_effects_if_needed(&mut self) {
        // Compact only after enough churn, keeping scans amortized rather than
        // removing one token by a full queue scan on every disposal.
        let limit = (self.active_count as usize)
            .saturating_mul(2)
            .saturating_add(64);
        if self.pending_effects.len() <= limit {
            return;
        }
        let nodes = &self.nodes;
        self.pending_effects.retain(|pending| {
            nodes
                .get(pending.id as usize)
                .is_some_and(|node| node.matches_queued_effect(pending.lifetime))
        });
    }

    pub fn pending_effect_count(&self) -> usize {
        self.pending_effects
            .iter()
            .filter(|pending| {
                self.nodes.get(pending.id as usize).is_some_and(|node| {
                    node.matches_queued_effect(pending.lifetime)
                        && self.node_states[pending.id as usize] != STATE_CLEAN
                })
            })
            .count()
    }

    /// Drains scheduled effects from matching live slot lifetimes into the uint32 flush buffer.
    pub fn prepare_flush(&mut self) -> usize {
        self.flush_buffer.clear();
        if self.batch_depth > 0 {
            return 0;
        }
        for pending in &self.pending_effects {
            if let Some(node) = self.nodes.get_mut(pending.id as usize)
                && node.matches_queued_effect(pending.lifetime)
            {
                node.in_queue = false;
                self.flush_buffer.push(pending.id);
            }
        }
        self.pending_effects.clear();
        self.flush_buffer.len()
    }
}
