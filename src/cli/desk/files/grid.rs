//! Grid view of a **Files** window (F2): cells of a glyph and a two-line name; a **Machine** cell
//! shows its status on the second line instead.

use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthChar;

use super::render::{item_style, ALERT, FOLDER};
use super::{Files, FilesHit, MachineRow};
use crate::cli::desk::menu::{fit, PANEL};
use crate::cli::desk::scroll::{self, Axis, Extent};

pub(super) const CELL_W: u16 = 14;
pub(super) const CELL_H: u16 = 3;

/// One cell: glyph, two text lines, style.
type Cell = (&'static str, String, String, Style);

pub(super) fn draw(
    files: &mut Files,
    rows: &[MachineRow],
    area: Rect,
    buf: &mut Buffer,
    push: &mut dyn FnMut(Rect, FilesHit),
) {
    files.bars = [None, None];
    files.hscroll = 0;
    if area.height < CELL_H || area.width < CELL_W {
        return;
    }
    let total = files.len(rows);
    files.page = usize::from(area.height / CELL_H);
    let across = |w: u16| usize::from((w / CELL_W).max(1));
    files.cols = across(area.width);
    let vbar = files.lines(rows) > files.page;
    let width = area.width - u16::from(vbar);
    files.cols = across(width);
    let lines = files.lines(rows);
    files.scroll = files.scroll.min(lines.saturating_sub(files.page));
    let first = files.scroll * files.cols;
    let count = files.page * files.cols;

    let cells: Vec<Cell> = if files.at_machines() {
        files
            .machine_order(rows)
            .into_iter()
            .map(|i| &rows[i])
            .skip(first)
            .take(count)
            .map(|r| {
                let glyph = if r.on { "◉" } else { "○" };
                let style = if r.alert { ALERT } else { Style::new() };
                (glyph, r.name.clone(), r.status.clone(), style)
            })
            .collect()
    } else {
        files
            .shown_entries()
            .skip(first)
            .take(count)
            .map(|e| {
                let (glyph, style) = if e.dir {
                    ("▣", FOLDER)
                } else {
                    ("▤", Style::new())
                };
                let (a, b) = wrap(&e.name, usize::from(CELL_W - 2));
                (glyph, a, b, style)
            })
            .collect()
    };
    let list = Rect::new(area.x, area.y, width, area.height);
    push(list, FilesHit::List);
    for (i, (glyph, a, b, style)) in cells.into_iter().enumerate() {
        let index = first + i;
        let (col, row) = ((i % files.cols) as u16, (i / files.cols) as u16);
        let rect = Rect::new(list.x + col * CELL_W, list.y + row * CELL_H, CELL_W, CELL_H);
        let style = item_style(files, index, style);
        let text = Rect::new(rect.x, rect.y, CELL_W - 1, CELL_H);
        buf.set_style(text, style);
        let w = usize::from(CELL_W - 2);
        buf.set_string(rect.x + 1 + (CELL_W - 2) / 2, rect.y, glyph, style);
        buf.set_stringn(rect.x + 1, rect.y + 1, &a, w, style);
        buf.set_stringn(rect.x + 1, rect.y + 2, &b, w, style);
        push(rect, FilesHit::Row(index));
        files.cells.push((rect, index));
    }
    if total == 0 && files.loading && !files.at_machines() {
        buf.set_string(list.x + 2, list.y, "loading…", PANEL);
    }
    if vbar {
        let track = Rect::new(area.right() - 1, area.y, 1, area.height);
        let ext = Extent::new(files.scroll, lines, files.page);
        scroll::draw(buf, Axis::Y, track, ext);
        files.bars[0] = Some((track, ext));
        push(track, FilesHit::Bar(Axis::Y));
    }
}

/// A name on two lines of `width` columns; the second is cut with `…`.
fn wrap(name: &str, width: usize) -> (String, String) {
    let mut first = String::new();
    let mut used = 0;
    let mut rest = name.char_indices();
    for (at, c) in rest.by_ref() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            return (first, fit(&name[at..], width));
        }
        used += w;
        first.push(c);
    }
    (first, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_splits_long_names_and_cuts_the_rest() {
        assert_eq!(wrap("short", 12), ("short".into(), String::new()));
        let (a, b) = wrap("a-rather-long-file-name-indeed.txt", 12);
        assert_eq!(a, "a-rather-lon");
        assert!(b.ends_with('…'), "{b}");
    }
}
