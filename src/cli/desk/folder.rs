//! **Machines Folder**: the **Native Window** listing Local and every saved **Machine** with its
//! connection state (ADR 0004 Tiers). Drawing only; app.rs owns the actions.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

use super::menu::{fit, FOCUS, MUTED, PANEL};
use super::wm::WinId;

pub(super) const TITLE: &str = "Machines";
const ICON: &str = " ▣ Machines ";

/// The open folder window and its highlighted row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Folder {
    pub win: WinId,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub name: String,
    pub target: String,
    pub status: String,
    /// `!`-states draw in the warning color.
    pub alert: bool,
}

/// Window size that fits `rows` (plus a header and a hint line).
pub(super) fn size(rows: &[Row]) -> (u16, u16) {
    let height = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .saturating_add(5);
    (72, height.clamp(8, 30))
}

/// The desktop icon at the top-left of the desk area; returns its rect.
pub(super) fn render_icon(desk: Rect, buf: &mut Buffer) -> Rect {
    let rect = Rect::new(
        desk.x + 1,
        desk.y,
        super::text_width(ICON).min(desk.width),
        1,
    )
    .intersection(desk);
    buf.set_stringn(
        rect.x,
        rect.y,
        ICON,
        usize::from(rect.width),
        Style::default().bg(Color::Indexed(25)).fg(Color::White),
    );
    rect
}

/// Draw the list into the window's content area; `push` reports each row's rect.
pub(super) fn render(
    rows: &[Row],
    selected: usize,
    inner: Rect,
    buf: &mut Buffer,
    mut push: impl FnMut(Rect, usize),
) {
    buf.set_style(inner, PANEL);
    if inner.height < 2 || inner.width < 20 {
        return;
    }
    let width = usize::from(inner.width);
    let (name_w, status_w) = (16, 18);
    let target_w = width.saturating_sub(name_w + status_w + 3);
    let line = |name: &str, target: &str, status: &str| {
        format!(
            " {:<name_w$}{:<target_w$} {:<status_w$}",
            fit(name, name_w - 1),
            fit(target, target_w.saturating_sub(1)),
            fit(status, status_w),
        )
    };
    buf.set_stringn(
        inner.x,
        inner.y,
        line("Name", "Target", "Status"),
        width,
        PANEL.patch(MUTED),
    );
    let list_h = usize::from(inner.height.saturating_sub(2));
    for (i, row) in rows.iter().enumerate().take(list_h) {
        let rect = Rect::new(inner.x, inner.y + 1 + i as u16, inner.width, 1);
        let mut style = if i == selected { FOCUS } else { PANEL };
        if row.alert && i != selected {
            style = style.fg(Color::LightRed);
        }
        buf.set_style(rect, style);
        buf.set_stringn(
            rect.x,
            rect.y,
            line(&row.name, &row.target, &row.status),
            width,
            style,
        );
        push(rect, i);
    }
    buf.set_stringn(
        inner.x,
        inner.bottom() - 1,
        " Double-click or Enter: connect · right-click: actions",
        width,
        PANEL.patch(MUTED),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> Row {
        Row {
            name: name.into(),
            target: "h".into(),
            status: "not connected".into(),
            alert: false,
        }
    }

    #[test]
    fn rows_report_hits_below_the_header() {
        let rows = [row("Local"), row("omarchy")];
        let inner = Rect::new(1, 1, 70, 6);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 10));
        let mut hits = Vec::new();
        render(&rows, 1, inner, &mut buf, |rect, i| hits.push((rect.y, i)));
        assert_eq!(hits, [(2, 0), (3, 1)]);
        assert_eq!(size(&rows), (72, 8));
    }
}
