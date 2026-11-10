//! Preorder forest with expansion and keyboard navigation over visible rows.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeItem {
    pub id: usize,
    pub parent: Option<usize>,
}

/// A preorder forest. Collapsing an ancestor moves a hidden selection to that
/// ancestor; keyboard movement visits visible rows only.
#[derive(Debug, Clone)]
pub struct TreeModel {
    items: Vec<TreeItem>,
    parents: HashMap<usize, Option<usize>>,
    expanded: HashSet<usize>,
    selected: Option<usize>,
}
impl TreeModel {
    pub fn new(items: Vec<TreeItem>) -> Result<Self, ModelError> {
        validate_ids(&items.iter().map(|item| item.id).collect::<Vec<_>>())?;
        let mut ancestors = Vec::new();
        for item in &items {
            match item.parent {
                None => ancestors.clear(),
                Some(parent) => {
                    let Some(index) = ancestors.iter().position(|id| *id == parent) else {
                        return Err(ModelError::InvalidTreeOrder(item.id));
                    };
                    ancestors.truncate(index + 1);
                }
            }
            ancestors.push(item.id);
        }
        let parents = items.iter().map(|item| (item.id, item.parent)).collect();
        Ok(Self {
            items,
            parents,
            expanded: HashSet::new(),
            selected: None,
        })
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn is_expanded(&self, id: usize) -> bool {
        self.expanded.contains(&id)
    }
    pub fn is_branch(&self, id: usize) -> bool {
        self.items.iter().any(|item| item.parent == Some(id))
    }
    pub fn visible_ids(&self) -> Vec<usize> {
        let mut visible = HashSet::new();
        let mut ids = Vec::new();
        for item in &self.items {
            if item
                .parent
                .is_none_or(|parent| visible.contains(&parent) && self.expanded.contains(&parent))
            {
                visible.insert(item.id);
                ids.push(item.id);
            }
        }
        ids
    }
    pub fn select(&mut self, id: usize) -> Result<bool, ModelError> {
        if !self.visible_ids().contains(&id) {
            return Err(ModelError::UnknownItem(id));
        }
        let changed = self.selected != Some(id);
        self.selected = Some(id);
        Ok(changed)
    }
    pub fn set_expanded(&mut self, id: usize, expanded: bool) -> Result<bool, ModelError> {
        if !self.parents.contains_key(&id) {
            return Err(ModelError::UnknownItem(id));
        }
        if !self.is_branch(id) {
            return Ok(false);
        }
        let changed = if expanded {
            self.expanded.insert(id)
        } else {
            self.expanded.remove(&id)
        };
        if !expanded {
            let mut ancestor = self.selected;
            while let Some(node) = ancestor {
                if node == id {
                    self.selected = Some(id);
                    break;
                }
                ancestor = self.parents[&node];
            }
        }
        Ok(changed)
    }
    pub fn toggle_expanded(&mut self, id: usize) -> Result<bool, ModelError> {
        self.set_expanded(id, !self.is_expanded(id))
    }
    pub fn move_by(&mut self, delta: isize) -> bool {
        let mut selection = SingleSelection::new(self.visible_ids(), self.selected).unwrap();
        let changed = selection.move_by(delta, false);
        self.selected = selection.selected();
        changed
    }
    pub fn first(&mut self) -> bool {
        self.visible_ids()
            .first()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
    pub fn last(&mut self) -> bool {
        self.visible_ids()
            .last()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
    pub fn right(&mut self) -> bool {
        let Some(id) = self.selected else {
            return self.first();
        };
        if !self.is_branch(id) {
            return false;
        }
        if !self.is_expanded(id) {
            return self.set_expanded(id, true).unwrap();
        }
        let child = self
            .items
            .iter()
            .find(|item| item.parent == Some(id))
            .unwrap()
            .id;
        self.select(child).unwrap()
    }
    pub fn left(&mut self) -> bool {
        let Some(id) = self.selected else {
            return false;
        };
        if self.is_expanded(id) {
            return self.set_expanded(id, false).unwrap();
        }
        self.parents[&id].is_some_and(|parent| self.select(parent).unwrap())
    }
}
