//! Modal dialog: optional one-line text input and an action row. Frame and action row ported
//! from wens-tui-lab `dialog.rs` (tui-components `modal-dialog.md`).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Clear, Widget},
};

use super::menu::{fit, ACCENT, FOCUS, MUTED, PANEL};
use super::text_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogButton {
    Primary,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Pending,
    Submit,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Dialog<T> {
    pub title: String,
    /// Shown when there is no input line (confirmation dialogs).
    pub message: String,
    pub input: Option<String>,
    pub primary: &'static str,
    pub destructive: bool,
    pub target: T,
}

impl<T> Dialog<T> {
    pub fn input(title: impl Into<String>, text: &str, primary: &'static str, target: T) -> Self {
        Self {
            title: title.into(),
            message: String::new(),
            input: Some(text.to_string()),
            primary,
            destructive: false,
            target,
        }
    }

    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        primary: &'static str,
        target: T,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            input: None,
            primary,
            destructive: true,
            target,
        }
    }

    /// One framed host input chunk: Enter submits, Esc cancels, Backspace and text edit.
    pub fn key(&mut self, chunk: &[u8]) -> Outcome {
        match chunk {
            b"\r" | b"\n" => Outcome::Submit,
            b"\x1b" => Outcome::Cancel,
            [0x7f] | [0x08] => {
                if let Some(text) = self.input.as_mut() {
                    text.pop();
                }
                Outcome::Pending
            }
            // Other escape sequences (arrows, function keys) are ignored.
            [0x1b, ..] => Outcome::Pending,
            _ => {
                if let (Some(text), Ok(typed)) = (self.input.as_mut(), std::str::from_utf8(chunk)) {
                    text.extend(typed.chars().filter(|c| !c.is_control()));
                }
                Outcome::Pending
            }
        }
    }

    pub fn rect(&self, screen: Rect) -> Rect {
        let width = 48.min(screen.width);
        let height = 5.min(screen.height);
        Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        )
    }

    /// Dim the screen, draw the dialog, report button hits.
    pub fn render(&self, screen: Rect, buf: &mut Buffer, mut push: impl FnMut(Rect, DialogButton)) {
        buf.set_style(screen, Style::new().add_modifier(Modifier::DIM));
        let rect = self.rect(screen);
        Clear.render(rect, buf);
        let block = Block::bordered()
            .title(format!(" {} ", self.title))
            .border_style(ACCENT.bg(Color::Indexed(236)))
            .style(PANEL);
        let inner = block.inner(rect);
        block.render(rect, buf);
        if inner.height < 2 || inner.width < 4 {
            return;
        }
        let line = Rect::new(inner.x + 1, inner.y, inner.width - 2, 1);
        match &self.input {
            Some(text) => {
                let field = Style::new().bg(Color::Indexed(238)).fg(Color::White);
                buf.set_style(line, field);
                // Keep the end of the text and the cursor visible.
                let room = usize::from(line.width.saturating_sub(1));
                let shown: String = {
                    let chars: Vec<char> = text.chars().collect();
                    let mut start = 0;
                    while usize::from(text_width(&chars[start..].iter().collect::<String>())) > room
                    {
                        start += 1;
                    }
                    chars[start..].iter().collect()
                };
                buf.set_string(line.x, line.y, &shown, field);
                let cursor_x = line.x + text_width(&shown);
                buf.set_string(cursor_x, line.y, "▏", field.fg(Color::Indexed(81)));
            }
            None => {
                buf.set_string(
                    line.x,
                    line.y,
                    fit(&self.message, usize::from(line.width)),
                    PANEL,
                );
            }
        }
        let row = Rect::new(line.x, inner.bottom() - 1, line.width, 1);
        let primary = format!(" ↵ {} ", self.primary);
        let cancel = " esc Cancel ".to_string();
        let pw = text_width(&primary).min(row.width);
        let cw = text_width(&cancel).min(row.width.saturating_sub(pw + 1));
        let p_rect = Rect::new(row.x, row.y, pw, 1);
        let c_rect = Rect::new(row.right().saturating_sub(cw), row.y, cw, 1);
        let p_style = if self.destructive {
            Style::new().bg(Color::LightRed).fg(Color::Black)
        } else {
            FOCUS
        };
        Span::styled(primary, p_style).render(p_rect, buf);
        Span::styled(cancel, PANEL.patch(MUTED)).render(c_rect, buf);
        push(p_rect, DialogButton::Primary);
        push(c_rect, DialogButton::Cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_editing_and_submit() {
        let mut d = Dialog::input("Rename", "ab", "Rename", ());
        assert_eq!(d.key(b"c\xe4\xb8\xad"), Outcome::Pending);
        assert_eq!(d.key(&[0x7f]), Outcome::Pending);
        assert_eq!(d.key(b"\x1b[A"), Outcome::Pending);
        assert_eq!(d.input.as_deref(), Some("abc"));
        assert_eq!(d.key(b"\r"), Outcome::Submit);
        assert_eq!(d.key(b"\x1b"), Outcome::Cancel);
    }

    #[test]
    fn buttons_are_reported_inside_the_dialog() {
        let screen = Rect::new(0, 0, 80, 24);
        let d = Dialog::confirm("Close", "Close task api?", "Close", ());
        let mut buf = Buffer::empty(screen);
        let mut hits = Vec::new();
        d.render(screen, &mut buf, |rect, b| hits.push((rect, b)));
        let rect = d.rect(screen);
        assert_eq!(hits.len(), 2);
        for (hit, _) in hits {
            assert!(rect.contains(hit.as_position()));
        }
    }
}
