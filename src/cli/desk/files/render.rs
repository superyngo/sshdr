//! Drawing a **Files** window: ribbon, address bar, list, status bar. Reports each clickable
//! rect through `push`, like the other desk widgets.

use std::time::SystemTime;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use super::{crumbs, Cmd, Files, FilesHit, Location, MachineRow, RibbonTab, SortKey};
use crate::cli::desk::menu::{fit, ACCENT, FOCUS, MUTED, PANEL};
use crate::cli::desk::scroll::{self, Axis, Extent};
use crate::cli::desk::text_width;

const BAR: Style = Style::new().bg(Color::Indexed(238)).fg(Color::White);
const FOLDER: Style = Style::new().fg(Color::LightBlue);
const ALERT: Style = Style::new().fg(Color::LightRed);
const FILTER_W: u16 = 18;

/// A ribbon command as drawn: label, command, enabled.
type Item = (String, Option<Cmd>, bool);

pub(in crate::cli::desk) fn render(
    files: &mut Files,
    rows: &[MachineRow],
    label: &dyn Fn(&str) -> String,
    inner: Rect,
    buf: &mut Buffer,
    push: &mut dyn FnMut(Rect, FilesHit),
) {
    buf.set_style(inner, PANEL);
    if inner.height < 5 || inner.width < 24 {
        return;
    }
    let mut y = inner.y;
    ribbon_tabs(files, Rect::new(inner.x, y, inner.width, 1), buf, push);
    y += 1;
    if !files.collapsed {
        let items = commands(files, rows);
        strip(&items, Rect::new(inner.x, y, inner.width, 1), buf, push);
        y += 1;
    }
    address(
        files,
        label,
        Rect::new(inner.x, y, inner.width, 1),
        buf,
        push,
    );
    y += 1;
    let status = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
    let list = Rect::new(inner.x, y, inner.width, status.y.saturating_sub(y));
    if list.height >= 2 {
        table(files, rows, list, buf, push);
    }
    status_bar(files, rows, status, buf);
}

fn ribbon_tabs(files: &Files, row: Rect, buf: &mut Buffer, push: &mut dyn FnMut(Rect, FilesHit)) {
    buf.set_style(row, BAR);
    let mut x = row.x + 1;
    for tab in RibbonTab::ALL {
        let text = format!(" {} ", tab.label());
        let w = text_width(&text);
        if x + w >= row.right() {
            break;
        }
        let style = if tab == files.tab { FOCUS } else { BAR };
        buf.set_string(x, row.y, &text, style);
        push(Rect::new(x, row.y, w, 1), FilesHit::Tab(tab));
        x += w + 1;
    }
    let glyph = if files.collapsed { "»" } else { "«" };
    let at = Rect::new(row.right() - 2, row.y, 1, 1);
    buf.set_string(at.x, at.y, glyph, BAR.fg(Color::Gray));
    push(at, FilesHit::Collapse);
}

fn commands(files: &Files, rows: &[MachineRow]) -> Vec<Item> {
    let item = |label: &str, cmd: Cmd, on: bool| (label.to_string(), Some(cmd), on);
    let gap = || (String::from("│"), None, false);
    let folder = files.folder().is_some();
    match files.tab {
        RibbonTab::Home => {
            let mut items = vec![
                item("Open", Cmd::Open, true),
                item("Open in terminal", Cmd::OpenTerminal, folder),
                item("Refresh", Cmd::Refresh, folder),
                gap(),
            ];
            for what in ["Cut", "Copy", "Paste", "Rename", "Trash", "New folder"] {
                items.push(item(what, Cmd::Later(what), false));
            }
            items
        }
        RibbonTab::View => {
            let view = files.view();
            let machines = files.at_machines();
            let mut items = vec![
                (String::from("• List"), None, false),
                item("Grid", Cmd::Later("Grid"), false),
                gap(),
            ];
            let keys: &[SortKey] = if machines {
                &[SortKey::Name, SortKey::Kind]
            } else {
                &SortKey::ALL
            };
            for &key in keys {
                let mark = if view.sort == key { "•" } else { " " };
                items.push(item(
                    &format!("{mark}{}", key.label(machines)),
                    Cmd::Sort(key),
                    true,
                ));
            }
            let order = if view.desc { "▼ desc" } else { "▲ asc" };
            items.push(item(order, Cmd::Desc, true));
            items.push(gap());
            if !machines {
                let check = |on: bool| if on { "☑" } else { "☐" };
                items.push(item(
                    &format!("{} Folders first", check(view.folders_first)),
                    Cmd::FoldersFirst,
                    true,
                ));
                items.push(item(
                    &format!("{} Hidden", check(view.hidden)),
                    Cmd::Hidden,
                    true,
                ));
            }
            let all = if machines {
                "Make default"
            } else {
                "Apply to all folders"
            };
            items.push(item(all, Cmd::MakeDefault, true));
            items
        }
        RibbonTab::Machine => match files.machine(rows) {
            Some(row) => {
                let connect = if row.on {
                    item("Disconnect", Cmd::Disconnect, row.key != super::LOCAL_KEY)
                } else {
                    item("Connect", Cmd::Connect, true)
                };
                vec![
                    connect,
                    item(
                        "New window",
                        Cmd::NewWindow,
                        row.on || row.key != super::LOCAL_KEY,
                    ),
                    item("Show windows", Cmd::ShowWindows, row.on),
                ]
            }
            None => vec![(String::from("no Machine selected"), None, false)],
        },
    }
}

fn strip(items: &[Item], row: Rect, buf: &mut Buffer, push: &mut dyn FnMut(Rect, FilesHit)) {
    let mut x = row.x + 1;
    for (label, cmd, enabled) in items {
        let w = text_width(label);
        if x + w > row.right() {
            break;
        }
        let style = match (cmd, enabled) {
            (Some(_), true) => PANEL.patch(ACCENT),
            _ => PANEL.patch(MUTED),
        };
        buf.set_string(x, row.y, label, style);
        if let (Some(cmd), true) = (cmd, enabled) {
            push(Rect::new(x, row.y, w, 1), FilesHit::Cmd(*cmd));
        }
        x += w + 2;
    }
}

fn address(
    files: &Files,
    label: &dyn Fn(&str) -> String,
    row: Rect,
    buf: &mut Buffer,
    push: &mut dyn FnMut(Rect, FilesHit),
) {
    let nav = [
        ("‹", FilesHit::Back, files.can_back()),
        ("›", FilesHit::Forward, files.can_forward()),
        ("↑", FilesHit::Up, !files.at_machines()),
    ];
    let mut x = row.x + 1;
    for (glyph, hit, on) in nav {
        buf.set_string(x, row.y, glyph, if on { PANEL } else { PANEL.patch(MUTED) });
        if on {
            push(Rect::new(x, row.y, 1, 1), hit);
        }
        x += 2;
    }
    let filter_x = row.right().saturating_sub(FILTER_W + 1);
    let mut parts = crumbs(&files.location, label);
    // Too long: drop crumbs after `Machines` until the rest fits, marking the cut with `…`.
    let room = filter_x.saturating_sub(x + 1);
    let width =
        |parts: &[(String, Location)]| -> u16 { parts.iter().map(|p| text_width(&p.0) + 3).sum() };
    let mut skipped = 0;
    while parts.len() > 2 && width(&parts) + 4 > room {
        parts.remove(1);
        skipped += 1;
    }
    for (i, (name, _)) in parts.iter().enumerate() {
        let index = if i == 0 { 0 } else { i + skipped };
        if i > 0 {
            let sep = if i == 1 && skipped > 0 {
                " … › "
            } else {
                " › "
            };
            buf.set_string(x, row.y, sep, PANEL.patch(MUTED));
            x += text_width(sep);
        }
        let text = fit(name, usize::from(filter_x.saturating_sub(x + 1)));
        let w = text_width(&text);
        let style = if i + 1 == parts.len() {
            PANEL.add_modifier(Modifier::BOLD)
        } else {
            PANEL
        };
        buf.set_string(x, row.y, &text, style);
        push(Rect::new(x, row.y, w, 1), FilesHit::Crumb(index));
        x += w;
    }
    let boxed = Rect::new(filter_x, row.y, FILTER_W, 1);
    let style = if files.filtering { FOCUS } else { BAR };
    buf.set_style(boxed, style);
    let text = if files.filtering {
        format!("⌕ {}▏", files.filter)
    } else if files.filter.is_empty() {
        "⌕ filter".to_string()
    } else {
        format!("⌕ {}", files.filter)
    };
    buf.set_stringn(boxed.x, boxed.y, &text, usize::from(FILTER_W), style);
    push(boxed, FilesHit::Filter);
}

/// Column: title, width, sort key, right-aligned.
type Column = (&'static str, u16, Option<SortKey>, bool);

/// Name (folders) or Target (`Machines`) never gets narrower than this; a narrower list scrolls
/// sideways (B32).
const MIN_GROW: u16 = 24;

fn columns(machines: bool, width: u16) -> Vec<Column> {
    if machines {
        let rest = width.saturating_sub(18 + 22 + 2).max(MIN_GROW);
        vec![
            ("Name", 18, Some(SortKey::Name), false),
            ("Target", rest, None, false),
            ("Status", 22, Some(SortKey::Kind), false),
        ]
    } else {
        let rest = width.saturating_sub(9 + 17 + 10 + 2).max(MIN_GROW);
        vec![
            ("Name", rest, Some(SortKey::Name), false),
            ("Size", 9, Some(SortKey::Size), true),
            ("Modified", 17, Some(SortKey::Modified), false),
            ("Kind", 10, Some(SortKey::Kind), false),
        ]
    }
}

/// Header and rows; a vertical bar on the right and a horizontal one at the bottom when they
/// overflow. Drawn off screen at full content width, then the visible columns are copied.
fn table(
    files: &mut Files,
    rows: &[MachineRow],
    area: Rect,
    buf: &mut Buffer,
    push: &mut dyn FnMut(Rect, FilesHit),
) {
    let machines = files.at_machines();
    let total = files.len(rows);
    let content_w = |w: u16| 2 + columns(machines, w).iter().map(|c| c.1).sum::<u16>();
    // Rows below the header, and whether each bar shows.
    let mut height = area.height - 1;
    let mut vbar = total > usize::from(height);
    let mut width = area.width - u16::from(vbar);
    let mut hbar = content_w(width) > width;
    if hbar && height > 1 {
        height -= 1;
        vbar = total > usize::from(height);
        width = area.width - u16::from(vbar);
        hbar = content_w(width) > width;
    } else {
        hbar = false;
    }
    let full = content_w(width);
    let cols = columns(machines, width);
    files.page = usize::from(height);
    files.scroll = files.scroll.min(total.saturating_sub(files.page));
    let hext = Extent::new(files.hscroll, usize::from(full), usize::from(width));
    files.hscroll = files.hscroll.min(hext.max());
    let shift = files.hscroll as u16;
    let view = *files.view();

    let mut tmp = Buffer::empty(Rect::new(0, 0, full, height + 1));
    tmp.set_style(Rect::new(0, 0, full, 1), PANEL.patch(MUTED));
    tmp.set_style(Rect::new(0, 1, full, height), PANEL);
    let mut headers = Vec::new();
    let mut x = 1;
    for &(title, w, key, right) in &cols {
        let mark = match key {
            Some(k) if k == view.sort => {
                if view.desc {
                    " ▼"
                } else {
                    " ▲"
                }
            }
            _ => "",
        };
        let text = fit(&format!("{title}{mark}"), usize::from(w.saturating_sub(1)));
        let tx = if right {
            x + w.saturating_sub(text_width(&text) + 1)
        } else {
            x
        };
        tmp.set_string(
            tx,
            0,
            &text,
            PANEL.patch(MUTED).add_modifier(Modifier::BOLD),
        );
        if let Some(key) = key {
            headers.push((x, w, key));
        }
        x += w;
    }
    let lines: Vec<(Vec<String>, Style)> = if machines {
        files
            .machine_order(rows)
            .into_iter()
            .map(|i| &rows[i])
            .map(|r| {
                let glyph = if r.on { "◉ " } else { "○ " };
                let cells = vec![
                    format!("{glyph}{}", r.name),
                    r.target.clone(),
                    r.status.clone(),
                ];
                (cells, if r.alert { ALERT } else { Style::new() })
            })
            .skip(files.scroll)
            .take(files.page)
            .collect()
    } else {
        let offset = local_offset();
        files
            .shown_entries()
            .skip(files.scroll)
            .take(files.page)
            .map(|e| {
                let glyph = if e.dir { "▸ " } else { "  " };
                let name = if e.dir {
                    format!("{glyph}{}/", e.name)
                } else {
                    format!("{glyph}{}", e.name)
                };
                let size = if e.dir {
                    String::new()
                } else {
                    human_size(e.size)
                };
                let when = e.modified.map(|t| when(t, offset)).unwrap_or_default();
                let style = if e.dir { FOLDER } else { Style::new() };
                (vec![name, size, when, e.kind.clone()], style)
            })
            .collect()
    };
    let shown = lines.len();
    for (i, (cells, style)) in lines.into_iter().enumerate() {
        let index = files.scroll + i;
        let y = 1 + i as u16;
        let style = if index == files.cursor {
            FOCUS
        } else {
            PANEL.patch(style)
        };
        tmp.set_style(Rect::new(0, y, full, 1), style);
        let mut x = 1;
        for (cell, &(_, w, _, right)) in cells.iter().zip(&cols) {
            let text = fit(cell, usize::from(w.saturating_sub(1)));
            let tx = if right {
                x + w.saturating_sub(text_width(&text) + 1)
            } else {
                x
            };
            tmp.set_string(tx, y, &text, style);
            x += w;
        }
    }
    if files.loading && shown == 0 && !machines {
        tmp.set_string(2, 1, "loading…", PANEL.patch(MUTED));
    }
    // Copy the visible columns.
    for y in 0..=height {
        for x in 0..width {
            if let Some(cell) = tmp.cell((x + shift, y)) {
                buf[(area.x + x, area.y + y)] = cell.clone();
            }
        }
    }
    for (x, w, key) in headers {
        let left = x.max(shift);
        let right = (x + w).min(shift + width);
        if left < right {
            push(
                Rect::new(area.x + left - shift, area.y, right - left, 1),
                FilesHit::Header(key),
            );
        }
    }
    let list = Rect::new(area.x, area.y + 1, width, height);
    push(list, FilesHit::List);
    for i in 0..shown {
        let rect = Rect::new(list.x, list.y + i as u16, width, 1);
        push(rect, FilesHit::Row(files.scroll + i));
    }
    files.bars = [None, None];
    let vext = Extent::new(files.scroll, total, files.page);
    if vbar {
        let track = Rect::new(area.right() - 1, list.y, 1, height);
        buf.set_style(track, PANEL);
        scroll::draw(buf, Axis::Y, track, vext);
        files.bars[0] = Some((track, vext));
        push(track, FilesHit::Bar(Axis::Y));
    }
    if hbar {
        let track = Rect::new(area.x, list.bottom(), width, 1);
        buf.set_style(track, PANEL);
        scroll::draw(
            buf,
            Axis::X,
            track,
            Extent::new(files.hscroll, usize::from(full), usize::from(width)),
        );
        files.bars[1] = Some((
            track,
            Extent::new(files.hscroll, usize::from(full), usize::from(width)),
        ));
        push(track, FilesHit::Bar(Axis::X));
    }
}

fn status_bar(files: &Files, rows: &[MachineRow], row: Rect, buf: &mut Buffer) {
    buf.set_style(row, BAR);
    let view = files.view();
    let mut parts = Vec::new();
    let shown = files.len(rows);
    if files.at_machines() {
        parts.push(format!("{shown} machines"));
    } else {
        let total = files.entry_count();
        parts.push(if shown == total {
            format!("{total} items")
        } else {
            format!("{shown} of {total} items")
        });
        if let Some(e) = files.current_entry() {
            parts.push(if e.dir {
                "1 selected".to_string()
            } else {
                format!("1 selected ({})", human_size(e.size))
            });
        }
        if let Some(free) = files.free {
            parts.push(format!("{} free", human_size(free)));
        }
    }
    let order = if view.desc { "▼" } else { "▲" };
    parts.push(format!(
        "list · {}{order}",
        view.sort.label(files.at_machines())
    ));
    if let Some(m) = files.folder().and_then(|_| files.machine(rows)) {
        parts.push(m.status.clone());
    }
    if files.loading {
        parts.push("loading…".to_string());
    }
    let text = format!(" {}", parts.join(" · "));
    buf.set_stringn(row.x, row.y, &text, usize::from(row.width), BAR);
    if let Some(err) = &files.error {
        let used = text_width(&text) + 3;
        if used < row.width {
            let err = fit(&format!("! {err}"), usize::from(row.width - used));
            buf.set_string(row.x + used, row.y, err, BAR.fg(Color::LightRed));
        }
    }
}

pub(super) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// Seconds east of UTC, from the platform's local time now.
fn local_offset() -> i64 {
    let Some(local) = crate::platform::local_datetime() else {
        return 0;
    };
    let utc = time::OffsetDateTime::now_utc();
    let diff = local.assume_utc().unix_timestamp() - utc.unix_timestamp();
    // Round to the quarter hour: the two clocks were read a moment apart.
    (diff as f64 / 900.0).round() as i64 * 900
}

fn when(at: SystemTime, offset: i64) -> String {
    let secs = match at.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    };
    match time::OffsetDateTime::from_unix_timestamp(secs.saturating_add(offset)) {
        Ok(t) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute()
        ),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Listing, Views};
    use super::*;
    use crate::cli::desk::files::Entry;
    use crate::cli::desk::transport::LOCAL_KEY;

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(4300), "4.2 KB");
        assert_eq!(human_size(50 * 1024 * 1024), "50 MB");
    }

    #[test]
    fn rows_headers_and_ribbon_report_hits() {
        let mut f = Files::new(1, Location::home(LOCAL_KEY), Views::default());
        let generation = f.begin_listing().map_or(0, |(g, _)| g);
        let entry = |name: &str| Entry {
            name: name.into(),
            dir: false,
            size: 1,
            modified: None,
            kind: "file".into(),
        };
        f.listed(Listing {
            generation,
            entries: Ok(vec![entry("a"), entry("b")]),
            stamp: None,
            free: Some(1 << 30),
        });
        let inner = Rect::new(1, 1, 80, 12);
        let mut buf = Buffer::empty(Rect::new(0, 0, 82, 14));
        let mut hits = Vec::new();
        render(
            &mut f,
            &[],
            &|_| "Local".into(),
            inner,
            &mut buf,
            &mut |r, h| hits.push((r.y, h)),
        );
        // Ribbon tabs (1), commands (2), address (3), header (4), rows from 5.
        assert!(hits.contains(&(1, FilesHit::Tab(RibbonTab::View))));
        assert!(hits.contains(&(2, FilesHit::Cmd(Cmd::Open))));
        assert!(hits.contains(&(3, FilesHit::Crumb(2))));
        assert!(hits.contains(&(4, FilesHit::Header(SortKey::Size))));
        assert!(hits.contains(&(5, FilesHit::Row(0))) && hits.contains(&(6, FilesHit::Row(1))));
        assert_eq!(f.page, 7);
        f.collapsed = true;
        hits.clear();
        render(
            &mut f,
            &[],
            &|_| "Local".into(),
            inner,
            &mut buf,
            &mut |r, h| hits.push((r.y, h)),
        );
        assert!(hits.contains(&(4, FilesHit::Row(0))));
        // Narrower than the columns: a horizontal bar on the last list row, columns shift.
        let narrow = Rect::new(1, 1, 40, 12);
        hits.clear();
        render(
            &mut f,
            &[],
            &|_| "Local".into(),
            narrow,
            &mut buf,
            &mut |r, h| hits.push((r.y, h)),
        );
        assert!(hits.contains(&(11, FilesHit::Bar(Axis::X))));
        assert!(!hits.iter().any(|h| h.1 == FilesHit::Bar(Axis::Y)));
        f.hscroll_by(99);
        assert_eq!(f.hscroll, 62 - 40);
    }
}
