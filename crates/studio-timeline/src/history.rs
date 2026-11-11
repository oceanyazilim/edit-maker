//! Snapshot-based undo/redo. Timelines are small (kilobytes), so full snapshots
//! are simpler and more robust than inverse operations.

use crate::model::Timeline;

#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<(String, Timeline)>,
    redo: Vec<(String, Timeline)>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(200)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: vec![],
            redo: vec![],
            limit: limit.max(1),
        }
    }
    /// Record the state *before* an edit labelled `label`.
    pub fn record(&mut self, label: impl Into<String>, before: &Timeline) {
        self.undo.push((label.into(), before.clone()));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, current: &mut Timeline) -> Option<String> {
        let (label, prev) = self.undo.pop()?;
        let now = std::mem::replace(current, prev);
        self.redo.push((label.clone(), now));
        Some(label)
    }
    pub fn redo(&mut self, current: &mut Timeline) -> Option<String> {
        let (label, next) = self.redo.pop()?;
        let now = std::mem::replace(current, next);
        self.undo.push((label.clone(), now));
        Some(label)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
