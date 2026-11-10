//! Command palette filtering and selection.

/// Command entry registered in [`CommandPaletteModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandItem {
    pub id: String,
    pub title: String,
    pub category: String,
    pub shortcut: String,
}

/// Interactive state model for `CommandPalette`: fuzzy/substring filtering,
/// ranked matching, keyboard navigation, and command execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPaletteModel {
    commands: Vec<CommandItem>,
    query: String,
    filtered: Vec<usize>,
    selected: usize,
    open: bool,
}

impl CommandPaletteModel {
    pub fn new(commands: Vec<CommandItem>) -> Self {
        let filtered = (0..commands.len()).collect();
        Self {
            commands,
            query: String::new(),
            filtered,
            selected: 0,
            open: false,
        }
    }

    fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }
        let q = query.to_lowercase();
        let c = candidate.to_lowercase();
        if let Some(pos) = c.find(&q) {
            return Some(1000 - (pos as i32) * 5 - (c.len() as i32));
        }
        let mut score = 0i32;
        let mut c_chars = c.chars();
        for qc in q.chars() {
            loop {
                match c_chars.next() {
                    Some(cc) if cc == qc => {
                        score += 10;
                        break;
                    }
                    Some(_) => score -= 1,
                    None => return None,
                }
            }
        }
        Some(score)
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        let mut scored: Vec<(usize, i32)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| {
                let hay = format!("{} {} {}", cmd.category, cmd.title, cmd.id);
                Self::fuzzy_score(&self.query, &hay).map(|s| (idx, s))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        self.selected = 0;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered
    }

    pub fn move_selection(&mut self, delta: isize) -> Option<&CommandItem> {
        if self.filtered.is_empty() {
            return None;
        }
        let len = self.filtered.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.active_command()
    }

    pub fn active_command(&self) -> Option<&CommandItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.commands.get(idx))
    }
}
