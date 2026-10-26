//! Selection in a **Files** folder (F3a): click, Shift+click range, Ctrl+click or Space toggle,
//! `^A`, and a box dragged over empty space. Kept by name, so a re-sort or re-list keeps it.
//! The `Machines` root has no selection; its cursor row is the one shown selected.

use std::collections::BTreeSet;

use crossterm::event::KeyModifiers;
use ratatui::layout::{Position, Rect};

use super::{Entry, Files};

/// A box drag: where it started and the selection it adds to.
#[derive(Debug, Clone)]
pub(super) struct Boxing {
    from: (u16, u16),
    base: BTreeSet<String>,
    pub to: (u16, u16),
}

impl Boxing {
    pub fn rect(&self) -> Rect {
        let (x0, x1) = (self.from.0.min(self.to.0), self.from.0.max(self.to.0));
        let (y0, y1) = (self.from.1.min(self.to.1), self.from.1.max(self.to.1));
        Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }
}

impl Files {
    fn name_at(&self, index: usize) -> Option<String> {
        let &i = self.shown.get(index)?;
        Some(self.entries.get(i)?.name.clone())
    }

    /// Drawn as selected: a selected entry, or the cursor row at the `Machines` root.
    pub fn is_selected(&self, index: usize) -> bool {
        if self.at_machines() {
            return index == self.cursor;
        }
        self.name_at(index)
            .is_some_and(|name| self.selected.contains(&name))
    }

    /// Selected folder entries in shown order.
    pub fn selected_entries(&self) -> impl Iterator<Item = &Entry> {
        self.shown_entries()
            .filter(|e| self.selected.contains(&e.name))
    }

    pub fn select_click(&mut self, index: usize, mods: KeyModifiers) {
        self.cursor = index;
        if mods.contains(KeyModifiers::SHIFT) {
            let anchor = self.anchor.min(self.shown.len().saturating_sub(1));
            let (a, b) = (anchor.min(index), anchor.max(index));
            self.selected = (a..=b).filter_map(|i| self.name_at(i)).collect();
            return;
        }
        self.anchor = index;
        if mods.contains(KeyModifiers::CONTROL) {
            self.toggle(index);
        } else {
            self.selected = self.name_at(index).into_iter().collect();
        }
    }

    /// The cursor moved by key: it alone is selected.
    pub fn select_cursor(&mut self) {
        self.anchor = self.cursor;
        self.selected = self.name_at(self.cursor).into_iter().collect();
    }

    /// Space: toggle the cursor entry.
    pub fn toggle_cursor(&mut self) {
        self.anchor = self.cursor;
        self.toggle(self.cursor);
    }

    fn toggle(&mut self, index: usize) {
        if let Some(name) = self.name_at(index) {
            if !self.selected.remove(&name) {
                self.selected.insert(name);
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selected = self.shown_entries().map(|e| e.name.clone()).collect();
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Drop names a new listing or filter no longer shows.
    pub(super) fn prune_selection(&mut self) {
        let shown: BTreeSet<String> = self.shown_entries().map(|e| e.name.clone()).collect();
        self.selected.retain(|name| shown.contains(name));
    }

    /// A press on empty list space: a box starts; Ctrl adds to the selection.
    pub fn box_start(&mut self, at: (u16, u16), mods: KeyModifiers) {
        if self.at_machines() {
            return;
        }
        if !mods.contains(KeyModifiers::CONTROL) {
            self.selected.clear();
        }
        self.boxing = Some(Boxing {
            from: at,
            base: self.selected.clone(),
            to: at,
        });
    }

    /// The box grew or shrank: the base plus every shown item it touches.
    pub fn box_to(&mut self, at: (u16, u16)) {
        let Some(boxing) = self.boxing.as_mut() else {
            return;
        };
        boxing.to = at;
        let rect = boxing.rect();
        let mut selected = boxing.base.clone();
        for &(cell, index) in &self.cells {
            if cell.intersects(rect) || rect.contains(Position::new(cell.x, cell.y)) {
                selected.extend(self.name_at(index));
            }
        }
        self.selected = selected;
    }

    pub fn box_end(&mut self) {
        self.boxing = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, files_with};
    use super::*;

    fn picked(f: &Files) -> Vec<&str> {
        f.selected_entries().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn click_shift_ctrl_space_and_all() {
        let mut f = files_with((1..=5).map(|i| entry(&format!("f{i}"), false, 1)).collect());
        f.select_click(1, KeyModifiers::NONE);
        f.select_click(3, KeyModifiers::SHIFT);
        assert_eq!(picked(&f), ["f2", "f3", "f4"]);
        f.select_click(0, KeyModifiers::CONTROL);
        f.select_click(2, KeyModifiers::CONTROL);
        assert_eq!(picked(&f), ["f1", "f2", "f4"]);
        f.cursor = 4;
        f.toggle_cursor();
        assert_eq!(picked(&f).len(), 4);
        f.select_all();
        assert_eq!(picked(&f).len(), 5);
        f.select_click(4, KeyModifiers::NONE);
        assert_eq!(picked(&f), ["f5"]);
        assert!(f.is_selected(4) && !f.is_selected(0));
    }

    #[test]
    fn a_box_selects_the_rows_it_touches_and_ctrl_adds() {
        let mut f = files_with((1..=5).map(|i| entry(&format!("f{i}"), false, 1)).collect());
        f.cells = (0..5)
            .map(|i| (Rect::new(0, 10 + i, 30, 1), usize::from(i)))
            .collect();
        f.select_click(0, KeyModifiers::NONE);
        f.box_start((5, 12), KeyModifiers::CONTROL);
        f.box_to((8, 13));
        assert_eq!(picked(&f), ["f1", "f3", "f4"]);
        f.box_to((8, 12));
        assert_eq!(picked(&f), ["f1", "f3"]);
        f.box_end();
        f.box_start((5, 14), KeyModifiers::NONE);
        f.box_to((5, 14));
        assert_eq!(picked(&f), ["f5"]);
    }
}
