//! Native retained-document scheduling. Values remain in Document; the owned
//! Svet graph coalesces state changes into a render job and survives scene rebuilds.
use crate::form_document::Properties;
use std::collections::BTreeMap;
use svet_graph::context::{Context, Kind, Node, Telemetry};

pub(crate) struct DocumentGraph {
    graph: Context,
    render: Node,
    topology: Node,
    sources: BTreeMap<(usize, String), Node>,
    states: Vec<Properties>,
    pending: bool,
}
impl DocumentGraph {
    pub(crate) fn new(states: &[Properties]) -> Self {
        let mut graph = Context::new();
        let render = graph.create(Kind::Effect).expect("document render effect");
        let topology = graph.create(Kind::Signal).expect("document shape signal");
        let mut result = Self {
            graph,
            render,
            topology,
            sources: BTreeMap::new(),
            states: Vec::new(),
            pending: false,
        };
        result.reconcile(states);
        result.complete();
        result
    }
    fn track(&mut self) {
        let sources = &self.sources;
        let topology = &self.topology;
        self.graph
            .track(&self.render, |scope| {
                scope.read(topology).unwrap();
                for source in sources.values() {
                    scope.read(source).unwrap();
                }
            })
            .unwrap();
    }
    pub(crate) fn reconcile(&mut self, states: &[Properties]) -> bool {
        // Graph mutations are private to this scheduler. Equal model state
        // needs neither re-tracking nor another owned snapshot, but an
        // unacknowledged render must remain pending after a template failure.
        if self.states == states {
            return self.pending;
        }
        let mut changed = Vec::new();
        let retired: Vec<_> = self
            .sources
            .keys()
            .filter(|(index, name)| {
                states
                    .get(*index)
                    .and_then(|state| state.get(name))
                    .is_none()
            })
            .cloned()
            .collect();
        let mut shape_changed = !retired.is_empty() || self.states.len() != states.len();
        for key in retired {
            self.graph
                .dispose(&self.sources.remove(&key).unwrap())
                .unwrap();
        }
        for (index, state) in states.iter().enumerate() {
            for (name, value) in state {
                let key = (index, name.clone());
                let node = match self.sources.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        shape_changed = true;
                        entry.insert(
                            self.graph
                                .create(Kind::Signal)
                                .expect("document state signal"),
                        )
                    }
                };
                if self.states.get(index).and_then(|old| old.get(name)) != Some(value) {
                    changed.push(node.clone());
                }
            }
        }
        // New sources must be subscribed before their first notification.
        if shape_changed {
            self.track();
            changed.push(self.topology.clone());
        }
        self.graph
            .batch(|graph| {
                for source in changed {
                    graph.notify(&source).unwrap();
                }
            })
            .unwrap();
        self.pending |= !self.graph.take_effects().is_empty();
        self.states = states.to_vec();
        self.pending
    }
    pub(crate) fn complete(&mut self) {
        self.track();
        self.pending = false;
    }
    pub(crate) fn telemetry(&self) -> Telemetry {
        self.graph.telemetry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form_document::Value;
    #[test]
    fn empty_scope_topology_changes_schedule_and_retry() {
        let mut graph = DocumentGraph::new(&[]);
        assert!(!graph.reconcile(&[]));
        assert!(graph.reconcile(&[Properties::new()]));
        assert!(graph.reconcile(&[Properties::new()]));
        graph.complete();
        assert!(!graph.reconcile(&[Properties::new()]));
        assert!(graph.reconcile(&[]));
        graph.complete();
        assert!(!graph.reconcile(&[]));
        assert_eq!(graph.telemetry().nodes, 2);
        assert_eq!(graph.telemetry().edges, 1);
    }
    #[test]
    fn property_changes_retry_and_retirement_are_owned() {
        let mut state = Properties::new();
        state.insert("count".into(), Value::Bool(false));
        let mut graph = DocumentGraph::new(&[state.clone()]);
        assert!(!graph.reconcile(&[state.clone()]));
        state.insert("count".into(), Value::Bool(true));
        assert!(graph.reconcile(&[state.clone()]));
        assert!(
            graph.reconcile(&[state.clone()]),
            "unacknowledged render retries"
        );
        graph.complete();
        assert!(!graph.reconcile(&[state.clone()]));
        assert!(graph.reconcile(&[]));
        graph.complete();
        assert_eq!(graph.telemetry().nodes, 2);
        assert_eq!(graph.telemetry().edges, 1);
        assert!(!graph.reconcile(&[]));
    }
}
