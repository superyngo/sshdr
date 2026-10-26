//! Dragging selected **Files** items (F3c): onto a folder or another **Files** window moves them
//! (Ctrl at the drop copies), onto a pane types their shell-quoted paths (`pane send-text`).

use crossterm::event::{KeyModifiers, MouseEvent};
use ratatui::{buffer::Buffer, style::Style};

use super::overflow::NativeDrag;
use super::{App, Hit};
use crate::cli::desk::files::ops::Op;
use crate::cli::desk::files::{FilesHit, Request};
use crate::cli::desk::menu::FOCUS;
use crate::cli::desk::server;
use crate::cli::desk::wm::WinId;

impl App {
    /// A left press on a selected row or cell may start a drag of the selection.
    pub(super) fn files_drag_start(&mut self, win: WinId, index: usize, at: (u16, u16)) {
        let selected = self
            .files
            .get(&win)
            .is_some_and(|f| !f.at_machines() && f.is_selected(index));
        if selected {
            self.scroll_drag = Some(NativeDrag::Items { win, from: at, at });
        }
    }

    /// The label following the pointer while items are dragged.
    pub(super) fn draw_ghost(&self, buf: &mut Buffer) {
        let Some(NativeDrag::Items { win, from, at }) = self.scroll_drag else {
            return;
        };
        if from == at {
            return;
        }
        let count = self.files.get(&win).map_or(0, |f| f.targets().len());
        let text = format!(" {count} item{} ", if count == 1 { "" } else { "s" });
        let area = buf.area;
        let x = at.0.saturating_add(1).min(area.right().saturating_sub(1));
        if at.1 < area.bottom() {
            buf.set_stringn(
                x,
                at.1,
                &text,
                usize::from(area.right() - x),
                Style::new().patch(FOCUS),
            );
        }
    }

    /// Released after a drag: move or copy into the folder below, or type the paths into the pane.
    pub(super) fn files_drop(&mut self, win: WinId, mouse: MouseEvent) {
        let Some(paths) = self.files.get(&win).map(|f| f.targets()) else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        let into = match self.hits.hit(mouse.column, mouse.row) {
            Some(Hit::Files(other, hit)) => {
                let Some(f) = self.files.get(&other) else {
                    return;
                };
                let Some((_, dir)) = f.folder() else {
                    return;
                };
                match hit {
                    FilesHit::Row(index) => match f.shown_entries().nth(index) {
                        Some(e) if e.dir => dir.join(&e.name),
                        _ if other == win => return,
                        _ => dir.to_path_buf(),
                    },
                    _ if other == win => return,
                    _ => dir.to_path_buf(),
                }
            }
            Some(Hit::Content(id, pane)) => {
                let text = paths
                    .iter()
                    .map(|p| shell_quote(&p.to_string_lossy()))
                    .collect::<Vec<_>>()
                    .join(" ");
                return self.send_text(id, pane, &format!("{text} "));
            }
            _ => return,
        };
        if paths
            .iter()
            .any(|p| p == &into || p.parent() == Some(into.as_path()))
        {
            return;
        }
        let cut = !mouse.modifiers.contains(KeyModifiers::CONTROL);
        self.files_op_request(
            win,
            Request::Op(Op::Paste {
                paths,
                to: into,
                cut,
            }),
        );
    }

    fn send_text(&mut self, id: WinId, pane: usize, text: &str) {
        let Some(data) = self.data.get(&id) else {
            return;
        };
        let active = &data.workspace.active_tab_id;
        let Some(tab) = data.tabs.iter().find(|t| &t.tab_id == active) else {
            return;
        };
        let Some(pane_id) = tab.panes.get(pane).map(|p| p.pane_id.clone()) else {
            return;
        };
        let key = data.target.clone();
        if let Some(target) = self.target(&key) {
            server::send_text(target, &pane_id, text);
        }
    }
}

/// POSIX single quotes; plain words stay bare.
fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c));
    if plain {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_quoted_only_when_needed() {
        assert_eq!(shell_quote("/tmp/a.txt"), "/tmp/a.txt");
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
