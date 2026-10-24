//! sshdr desk client (ADR 0006): a local terminal that replays one session stream's ANSI
//! frames and renders them with the pane renderer, without widening `GhosttyPaneTerminal`.

use bytes::Bytes;
use ratatui::{layout::Rect, Frame};
use tokio::sync::mpsc;

use super::terminal::GhosttyPaneTerminal;
use crate::layout::PaneId;

pub(crate) struct DeskMirror {
    terminal: GhosttyPaneTerminal,
    // Frames carry no terminal queries, so responses are dropped.
    responses: mpsc::Sender<Bytes>,
    cols: u16,
    rows: u16,
}

impl DeskMirror {
    pub(crate) fn new(cols: u16, rows: u16) -> std::io::Result<Self> {
        let (responses, _) = mpsc::channel(1);
        let cols = cols.max(1);
        let rows = rows.max(1);
        let raw = crate::ghostty::Terminal::new(cols, rows, 0)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let terminal = GhosttyPaneTerminal::new(raw, responses.clone())?;
        Ok(Self {
            terminal,
            responses,
            cols,
            rows,
        })
    }

    /// Apply one frame; the stream's frame size wins over the local size.
    pub(crate) fn feed(&mut self, width: u16, height: u16, bytes: &[u8]) {
        if width > 0 && height > 0 && (width, height) != (self.cols, self.rows) {
            self.cols = width;
            self.rows = height;
            let _ = self.terminal.resize(height, width, 0, 0);
        }
        let _ = self
            .terminal
            .process_pty_bytes(PaneId::from_raw(0), 0, bytes, &self.responses);
    }

    pub(crate) fn render(&self, frame: &mut Frame, area: Rect, show_cursor: bool) {
        self.terminal.render(frame, area, show_cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn replayed_frame_renders_into_area_offset() {
        let mut mirror = DeskMirror::new(6, 2).expect("mirror");
        mirror.feed(6, 2, b"\x1b[2J\x1b[1;1Hhi\x1b[2;3H\x1b[31mok\x1b[0m");
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        terminal
            .draw(|frame| mirror.render(frame, Rect::new(2, 1, 6, 2), false))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 1)].symbol(), "h");
        assert_eq!(buffer[(3, 1)].symbol(), "i");
        assert_eq!(buffer[(4, 2)].symbol(), "o");
        assert_eq!(buffer[(5, 2)].symbol(), "k");
        assert_eq!(buffer[(4, 2)].fg, ratatui::style::Color::Indexed(1));
    }

    #[test]
    fn frame_size_change_resizes_mirror() {
        let mut mirror = DeskMirror::new(4, 2).expect("mirror");
        mirror.feed(8, 3, b"\x1b[2J\x1b[3;8Hz");
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).expect("terminal");
        terminal
            .draw(|frame| mirror.render(frame, Rect::new(0, 0, 8, 3), false))
            .expect("draw");
        assert_eq!(terminal.backend().buffer()[(7, 2)].symbol(), "z");
    }
}
