//! Popup / context menu, ported from wens-tui-lab `menu.rs` (tui-components `menu-dropdown.md`).

use super::scroll::{self, Axis, Extent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};

use super::text_width;

pub(super) const PANEL: Style = Style::new().bg(Color::Indexed(236)).fg(Color::White);
pub(super) const ACCENT: Style = Style::new().fg(Color::Indexed(81));
pub(super) const FOCUS: Style = Style::new().bg(Color::Indexed(81)).fg(Color::Black);
pub(super) const MUTED: Style = Style::new().fg(Color::Indexed(245));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuItem<A> {
    pub label: String,
    pub hint: &'static str,
    pub action: A,
}

impl<A> MenuItem<A> {
    pub fn new(label: impl Into<String>, hint: &'static str, action: A) -> Self {
        Self {
            label: label.into(),
            hint,
            action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Menu<A> {
    pub title: String,
    /// Top-left corner (pointer cell); `above` puts the bottom edge there instead.
    pub at: (u16, u16),
    pub above: bool,
    pub items: Vec<MenuItem<A>>,
    pub highlighted: usize,
    /// First item shown when the items overflow the screen (B32).
    pub top: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuHit {
    Row(usize),
    Body,
    /// The scrollbar on the right border.
    Bar,
}

impl<A: Clone> Menu<A> {
    /// `None` when there are no items.
    pub fn open(title: impl Into<String>, at: (u16, u16), items: Vec<MenuItem<A>>) -> Option<Self> {
        if items.is_empty() {
            return None;
        }
        Some(Self {
            title: title.into(),
            at,
            above: false,
            items,
            highlighted: 0,
            top: 0,
        })
    }

    pub fn above(mut self) -> Self {
        self.above = true;
        self
    }

    /// Move by `delta`, wrapping.
    pub fn step(&mut self, delta: isize) {
        let len = self.items.len() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
    }

    /// Rows that fit inside the border.
    fn visible(&self, screen: Rect) -> usize {
        self.rect(screen)
            .map_or(1, |r| usize::from(r.height.saturating_sub(2)).max(1))
    }

    /// First shown item: `top`, moved just enough to show the highlighted one.
    fn first(&self, visible: usize) -> usize {
        let low = self.highlighted.saturating_sub(visible.saturating_sub(1));
        self.top
            .clamp(low, self.highlighted)
            .min(self.items.len().saturating_sub(visible))
    }

    /// The vertical extent for the scrollbar.
    pub fn extent(&self, screen: Rect) -> Extent {
        let visible = self.visible(screen);
        Extent::new(self.first(visible), self.items.len(), visible)
    }

    /// The scrollbar track (right border rows), when the items overflow.
    pub fn bar(&self, screen: Rect) -> Option<Rect> {
        let rect = self.rect(screen)?;
        self.extent(screen).overflows().then(|| {
            Rect::new(
                rect.right() - 1,
                rect.y + 1,
                1,
                rect.height.saturating_sub(2),
            )
        })
    }

    /// Show from `top` (wheel, scrollbar); the highlight stays on a shown item.
    pub fn scroll_to(&mut self, top: usize, screen: Rect) {
        let visible = self.visible(screen);
        self.top = top.min(self.items.len().saturating_sub(visible));
        self.highlighted = self
            .highlighted
            .clamp(self.top, self.top + visible.saturating_sub(1));
    }

    pub fn scroll_by(&mut self, delta: isize, screen: Rect) {
        let first = self.first(self.visible(screen));
        self.scroll_to(first.saturating_add_signed(delta), screen);
    }

    pub fn action(&self, index: usize) -> Option<A> {
        self.items.get(index).map(|i| i.action.clone())
    }

    pub fn rect(&self, screen: Rect) -> Option<Rect> {
        if screen.width < 3 || screen.height < 3 {
            return None;
        }
        let label = self
            .items
            .iter()
            .map(|i| text_width(&i.label) + gap(i.hint))
            .max()
            .unwrap_or(0);
        let width = label
            .max(text_width(&self.title) + 2)
            .saturating_add(4)
            .max(14);
        let height = u16::try_from(self.items.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2);
        let (x, y) = self.at;
        let y = if self.above {
            y.saturating_sub(height)
        } else {
            y
        };
        Some(clamp_into(Rect::new(x, y, width, height), screen))
    }

    /// Draw and report hits: `Body` for the whole rect, then `Row(i)` per visible row.
    pub fn render(&self, screen: Rect, buf: &mut Buffer, mut push: impl FnMut(Rect, MenuHit)) {
        let Some(rect) = self.rect(screen) else {
            return;
        };
        Clear.render(rect, buf);
        let block = Block::bordered()
            .title(format!(" {} ", self.title))
            .border_style(ACCENT.bg(Color::Indexed(236)))
            .style(PANEL);
        let inner = block.inner(rect);
        block.render(rect, buf);
        push(rect, MenuHit::Body);
        let visible = usize::from(inner.height);
        let first = self.first(visible);
        for (row, (index, item)) in self
            .items
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .enumerate()
        {
            let row_rect = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            let hint_w = text_width(item.hint);
            let label_w = usize::from(inner.width.saturating_sub(hint_w + 2));
            let style = if index == self.highlighted {
                FOCUS
            } else {
                PANEL
            };
            let label: String = fit(&item.label, label_w);
            let mut spans = vec![Span::styled(format!(" {label}"), style)];
            if hint_w > 0 && inner.width > hint_w + 2 {
                let pad = label_w.saturating_sub(usize::from(text_width(&label)));
                spans.push(Span::styled(" ".repeat(pad), style));
                let hint_style = if index == self.highlighted {
                    style
                } else {
                    style.patch(MUTED)
                };
                spans.push(Span::styled(format!(" {} ", item.hint), hint_style));
            }
            Line::from(spans).style(style).render(row_rect, buf);
            push(row_rect, MenuHit::Row(index));
        }
        if let Some(track) = self.bar(screen) {
            scroll::draw(buf, Axis::Y, track, self.extent(screen));
            push(track, MenuHit::Bar);
        }
    }
}

fn gap(hint: &str) -> u16 {
    if hint.is_empty() {
        0
    } else {
        text_width(hint) + 2
    }
}

/// Cut `text` to `width` columns with a trailing `…`.
pub(super) fn fit(text: &str, width: usize) -> String {
    if usize::from(text_width(text)) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut tmp = [0; 4];
        if usize::from(text_width(&out) + text_width(ch.encode_utf8(&mut tmp))) + 1 > width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

pub(super) fn clamp_into(want: Rect, screen: Rect) -> Rect {
    let width = want.width.min(screen.width);
    let height = want.height.min(screen.height);
    let max_x = screen.right().saturating_sub(width);
    let max_y = screen.bottom().saturating_sub(height);
    Rect::new(
        want.x.clamp(screen.x, max_x),
        want.y.clamp(screen.y, max_y),
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<MenuItem<u8>> {
        vec![
            MenuItem::new("one", "", 1),
            MenuItem::new("two", "", 2),
            MenuItem::new("ＷＩＤＥ", "x", 3),
        ]
    }

    #[test]
    fn long_menus_scroll_and_keep_the_highlight_shown() {
        let items = (0..20)
            .map(|i| MenuItem::new(format!("i{i}"), "", i))
            .collect();
        let screen = Rect::new(0, 0, 30, 7);
        let mut m = Menu::open("t", (0, 0), items).unwrap();
        assert!(m.bar(screen).is_some());
        m.scroll_by(3, screen);
        assert_eq!((m.extent(screen).offset, m.highlighted), (3, 3));
        m.scroll_to(99, screen);
        assert_eq!((m.extent(screen).offset, m.highlighted), (15, 15));
        m.step(-1);
        assert_eq!(m.extent(screen).offset, 14);
        let mut buf = Buffer::empty(screen);
        let mut rows = Vec::new();
        m.render(screen, &mut buf, |_, h| rows.push(h));
        assert!(rows.contains(&MenuHit::Row(14)) && rows.contains(&MenuHit::Bar));
    }

    #[test]
    fn steps_wrap() {
        let mut m = Menu::open("t", (0, 0), items()).unwrap();
        m.step(-1);
        assert_eq!(m.highlighted, 2);
        m.step(1);
        assert_eq!(m.action(m.highlighted), Some(1));
        assert!(Menu::<u8>::open("t", (0, 0), vec![]).is_none());
    }

    #[test]
    fn menus_stay_on_screen_and_open_above_the_bar() {
        let screen = Rect::new(0, 0, 40, 10);
        let m = Menu::open("t", (39, 9), items()).unwrap();
        let r = m.rect(screen).unwrap();
        assert!(r.right() <= screen.right() && r.bottom() <= screen.bottom());
        assert_eq!(r.height, 5);
        let up = Menu::open("t", (2, 10), items()).unwrap().above();
        assert_eq!(up.rect(screen).unwrap().bottom(), 10);
    }

    #[test]
    fn rows_report_hits_in_order() {
        let screen = Rect::new(0, 0, 40, 10);
        let m = Menu::open("t", (1, 1), items()).unwrap();
        let mut buf = Buffer::empty(screen);
        let mut hits = Vec::new();
        m.render(screen, &mut buf, |rect, hit| hits.push((rect, hit)));
        assert_eq!(hits[0].1, MenuHit::Body);
        assert_eq!(
            hits[1],
            (Rect::new(2, 2, hits[1].0.width, 1), MenuHit::Row(0))
        );
        assert_eq!(hits.len(), 4);
    }
}
