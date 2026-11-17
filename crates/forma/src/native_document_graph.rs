//! Native retained-document scheduling. Values remain in Document; the owned
//! Svet graph coalesces state changes into a render job and survives scene rebuilds.
use crate::form_document::Properties;
use std::collections::BTreeMap;
use svet_graph::context::{Context, Kind, Node, Telemetry};

pub(crate) struct DocumentGraph {
    graph: Context,
    render: Node,
    topology: Node,
    sources: Vec<BTreeMap<String, Node>>,
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
            sources: Vec::new(),
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
                for scope_sources in sources {
                    for source in scope_sources.values() {
                        scope.read(source).unwrap();
                    }
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
        let mut shape_changed = self.states.len() != states.len();
        while self.sources.len() > states.len() {
            shape_changed = true;
            if let Some(scope_sources) = self.sources.pop() {
                for node in scope_sources.into_values() {
                    self.graph.dispose(&node).unwrap();
                }
            }
        }
        for (index, scope_sources) in self.sources.iter_mut().enumerate() {
            let state = &states[index];
            let retired: Vec<String> = scope_sources
                .keys()
                .filter(|name| state.get(name.as_str()).is_none())
                .cloned()
                .collect();
            if !retired.is_empty() {
                shape_changed = true;
                for name in retired {
                    if let Some(node) = scope_sources.remove(&name) {
                        self.graph.dispose(&node).unwrap();
                    }
                }
            }
        }
        if self.sources.len() < states.len() {
            self.sources.resize_with(states.len(), BTreeMap::new);
        }
        for (index, state) in states.iter().enumerate() {
            for (name, value) in state {
                let node = if let Some(existing) = self.sources[index].get(name.as_str()) {
                    existing
                } else {
                    shape_changed = true;
                    let created = self
                        .graph
                        .create(Kind::Signal)
                        .expect("document state signal");
                    self.sources[index].entry(name.clone()).or_insert(created)
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
