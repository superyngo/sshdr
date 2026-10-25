//! **Taskbar** geometry (B21): which screen edge it sits on, its size, and where the Start
//! button, the **Task** buttons, and the hint go. Pure layout; `app.rs` draws and routes hits.

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use super::text_width;

/// Widest Task label shown before it is cut with `…`.
const MAX_LABEL: u16 = 16;
pub(super) const START: &str = " ⊞ Start ";
/// Rows of a top or bottom **Taskbar**.
pub(super) const ROWS: (u16, u16) = (1, 4);
/// Columns of a left or right **Taskbar**.
pub(super) const COLUMNS: (u16, u16) = (12, 40);
/// Columns of a collapsed left or right **Taskbar** (herdr's compact sidebar width).
pub(super) const COLLAPSED: u16 = 4;
/// Start button of a collapsed **Taskbar**.
pub(super) const START_SHORT: &str = " ⊞ ";
/// Desk space always left beside the **Taskbar**.
const MIN_DESK: (u16, u16) = (20, 6);
/// Shortest hint worth drawing.
const MIN_HINT: u16 = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Edge {
    #[default]
    Bottom,
    Top,
    Left,
    Right,
}

impl Edge {
    pub fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }

    pub fn label(self) -> &'static str {
        match self {
            Edge::Bottom => "Bottom",
            Edge::Top => "Top",
            Edge::Left => "Left",
            Edge::Right => "Right",
        }
    }
}

/// Where the **Taskbar** is: saved in `desk.json` as `taskbar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Place {
    pub edge: Edge,
    /// Rows (top, bottom) or columns (left, right).
    pub size: u16,
    /// A left or right bar shrunk to [`COLLAPSED`] columns (B30); `size` is kept for expanding.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            edge: Edge::Bottom,
            size: ROWS.0,
            collapsed: false,
        }
    }
}

impl Place {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The same edge with `size` clamped to its range and to what `screen` leaves for the desk.
    pub fn sized(self, size: u16, screen: Rect) -> Self {
        let (min, max, room) = if self.edge.vertical() {
            (
                COLUMNS.0,
                COLUMNS.1,
                screen.width.saturating_sub(MIN_DESK.0),
            )
        } else {
            (ROWS.0, ROWS.1, screen.height.saturating_sub(MIN_DESK.1))
        };
        Self {
            edge: self.edge,
            size: size.clamp(min, max).min(room.max(min)),
            collapsed: self.collapsed,
        }
    }

    /// `edge`, keeping the size when the orientation stays, else the new orientation's default.
    pub fn moved(self, edge: Edge, screen: Rect) -> Self {
        let size = match (self.edge.vertical(), edge.vertical()) {
            (a, b) if a == b => self.size,
            (_, true) => 24,
            (_, false) => ROWS.0,
        };
        Self {
            edge,
            size,
            collapsed: self.collapsed,
        }
        .sized(size, screen)
    }

    /// Collapsed and on a side: drawn [`COLLAPSED`] columns wide.
    pub fn is_collapsed(self) -> bool {
        self.collapsed && self.edge.vertical()
    }

    /// The **Taskbar** rectangle; empty when the screen is too small for it.
    pub fn bar(self, screen: Rect) -> Rect {
        let size = if self.is_collapsed() {
            COLLAPSED
        } else {
            self.sized(self.size, screen).size
        };
        let rect = match self.edge {
            Edge::Bottom => Rect::new(
                screen.x,
                screen.bottom().saturating_sub(size),
                screen.width,
                size,
            ),
            Edge::Top => Rect::new(screen.x, screen.y, screen.width, size),
            Edge::Left => Rect::new(screen.x, screen.y, size, screen.height),
            Edge::Right => Rect::new(
                screen.right().saturating_sub(size),
                screen.y,
                size,
                screen.height,
            ),
        };
        rect.intersection(screen)
    }

    /// The desk: the screen without the **Taskbar**.
    pub fn desk(self, screen: Rect) -> Rect {
        let bar = self.bar(screen);
        match self.edge {
            Edge::Bottom => Rect::new(screen.x, screen.y, screen.width, screen.height - bar.height),
            Edge::Top => Rect::new(
                screen.x,
                bar.bottom(),
                screen.width,
                screen.height - bar.height,
            ),
            Edge::Left => Rect::new(
                bar.right(),
                screen.y,
                screen.width - bar.width,
                screen.height,
            ),
            Edge::Right => Rect::new(screen.x, screen.y, screen.width - bar.width, screen.height),
        }
    }

    /// The desk row or column touching the **Taskbar**: dragging it resizes the bar.
    pub fn grip(self, screen: Rect) -> Rect {
        if self.is_collapsed() {
            return Rect::default();
        }
        let desk = self.desk(screen);
        let rect = match self.edge {
            Edge::Bottom => Rect::new(desk.x, desk.bottom().saturating_sub(1), desk.width, 1),
            Edge::Top => Rect::new(desk.x, desk.y, desk.width, 1),
            Edge::Left => Rect::new(desk.x, desk.y, 1, desk.height),
            Edge::Right => Rect::new(desk.right().saturating_sub(1), desk.y, 1, desk.height),
        };
        rect.intersection(desk)
    }

    /// Size for a grip dragged to `pointer`.
    pub fn resized(self, pointer: (u16, u16), screen: Rect) -> Self {
        let (column, row) = pointer;
        let size = match self.edge {
            Edge::Bottom => screen.bottom().saturating_sub(row),
            Edge::Top => row.saturating_sub(screen.y) + 1,
            Edge::Left => column.saturating_sub(screen.x) + 1,
            Edge::Right => screen.right().saturating_sub(column),
        };
        self.sized(size, screen)
    }
}

/// The screen edge nearest to `pointer`, measured as a fraction of the screen so a wide
/// terminal does not favour top and bottom.
pub(super) fn nearest_edge(pointer: (u16, u16), screen: Rect) -> Edge {
    let (column, row) = pointer;
    let fx = f32::from(column.saturating_sub(screen.x)) / f32::from(screen.width.max(1));
    let fy = f32::from(row.saturating_sub(screen.y)) / f32::from(screen.height.max(1));
    [
        (fy, Edge::Top),
        (1.0 - fy, Edge::Bottom),
        (fx, Edge::Left),
        (1.0 - fx, Edge::Right),
    ]
    .into_iter()
    .min_by(|a, b| a.0.total_cmp(&b.0))
    .map_or(Edge::Bottom, |(_, edge)| edge)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Button {
    pub rect: Rect,
    pub text: String,
}

/// Where everything on the **Taskbar** goes. `buttons[i]` is the i-th **Task**; Tasks that do
/// not fit are left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Layout {
    pub start: Rect,
    pub buttons: Vec<Button>,
    pub hint: Option<Rect>,
    /// `«` (expanded) or `»` (collapsed) on the last row of a side bar.
    pub toggle: Option<Rect>,
}

/// Button text for a **Task**: label (cut to [`MAX_LABEL`]) and its window count.
pub(super) fn task_text(label: &str, windows: usize) -> String {
    let mut cut = String::new();
    for ch in label.chars() {
        if text_width(&cut) + text_width(ch.encode_utf8(&mut [0; 4])) > MAX_LABEL - 1 {
            cut.push('…');
            break;
        }
        cut.push(ch);
    }
    format!(" {cut} ·{windows} ")
}

/// Button text for a **Task** on a collapsed bar: its first character and window count.
pub(super) fn task_short(label: &str, windows: usize) -> String {
    let first = label.chars().next().unwrap_or(' ');
    format!(" {first}{windows}")
}

/// Lay out the **Taskbar** `bar` on `edge`.
/// - Top or bottom: Start, then the buttons left to right with one gap column, wrapping to the
///   next row; the hint takes the rest of the last used row, or the next free row.
/// - Left or right: Start on the first row, one button per row below it (cut to the width), the
///   hint and the collapse toggle on the last row; a `collapsed` bar has only the toggle there.
pub(super) fn layout(
    bar: Rect,
    edge: Edge,
    collapsed: bool,
    texts: impl IntoIterator<Item = String>,
) -> Layout {
    let collapsed = collapsed && edge.vertical();
    let start_text = if collapsed { START_SHORT } else { START };
    let start_width = text_width(start_text).min(bar.width);
    let start = Rect::new(bar.x, bar.y, start_width, bar.height.min(1));
    let mut buttons = Vec::new();
    if bar.is_empty() {
        return Layout {
            start,
            buttons,
            hint: None,
            toggle: None,
        };
    }
    if edge.vertical() {
        let last = bar.bottom() - 1;
        for (y, text) in (bar.y + 1..last).zip(texts) {
            let text = super::menu::fit(&text, usize::from(bar.width));
            let width = text_width(&text);
            buttons.push(Button {
                rect: Rect::new(bar.x, y, width, 1),
                text,
            });
        }
        let has_last = last > bar.y;
        // herdr's sidebar: `«` two columns from the right, `»` centred when collapsed.
        let toggle = (has_last && bar.width > 1).then(|| {
            let x = if collapsed {
                bar.x + bar.width / 2
            } else {
                bar.right() - 2
            };
            Rect::new(x, last, 1, 1)
        });
        let hint = (has_last && !collapsed && bar.width > 3)
            .then(|| Rect::new(bar.x + 1, last, bar.width - 3, 1));
        return Layout {
            start,
            buttons,
            hint,
            toggle,
        };
    }
    let mut x = start.right() + 2;
    let mut y = bar.y;
    for text in texts {
        let width = text_width(&text);
        if x.saturating_add(width) > bar.right() {
            if y + 1 >= bar.bottom() || bar.x + 1 + width > bar.right() {
                break;
            }
            y += 1;
            x = bar.x + 1;
        }
        buttons.push(Button {
            rect: Rect::new(x, y, width, 1),
            text,
        });
        x = x.saturating_add(width + 1);
    }
    let hint = if bar.right().saturating_sub(x) >= MIN_HINT {
        Some(Rect::new(x, y, bar.right() - x, 1))
    } else if y + 1 < bar.bottom() {
        Some(Rect::new(bar.x + 1, y + 1, bar.width - 1, 1))
    } else {
        None
    };
    Layout {
        start,
        buttons,
        hint,
        toggle: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 120, 36);

    fn texts(n: usize) -> Vec<String> {
        (0..n).map(|i| task_text(&format!("task{i}"), 1)).collect()
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.intersection(outer) == inner
    }

    #[test]
    fn long_labels_are_cut_and_counted() {
        assert_eq!(task_text("api", 2), " api ·2 ");
        let text = task_text("a-very-long-workspace-name", 1);
        assert!(text.contains('…'));
        assert!(text_width(&text) <= MAX_LABEL + 5);
    }

    #[test]
    fn every_edge_splits_the_screen_into_bar_desk_and_grip() {
        for edge in [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right] {
            let place = Place::default().moved(edge, SCREEN);
            let (bar, desk, grip) = (place.bar(SCREEN), place.desk(SCREEN), place.grip(SCREEN));
            assert_eq!(bar.intersection(desk).area(), 0, "{edge:?}");
            assert_eq!(bar.area() + desk.area(), SCREEN.area(), "{edge:?}");
            assert!(inside(grip, desk), "{edge:?}");
            // The grip touches the bar.
            let touching = match edge {
                Edge::Bottom => grip.bottom() == bar.y,
                Edge::Top => grip.y == bar.bottom(),
                Edge::Left => grip.x == bar.right(),
                Edge::Right => grip.right() == bar.x,
            };
            assert!(touching, "{edge:?}");
        }
        let bottom = Place::default();
        assert_eq!(bottom.bar(SCREEN), Rect::new(0, 35, 120, 1));
        assert_eq!(bottom.desk(SCREEN), Rect::new(0, 0, 120, 35));
    }

    #[test]
    fn every_edge_lays_out_start_buttons_and_hint_inside_the_bar() {
        for (edge, size) in [
            (Edge::Bottom, 1),
            (Edge::Bottom, 3),
            (Edge::Top, 2),
            (Edge::Left, 16),
            (Edge::Right, 30),
        ] {
            let place = Place {
                edge,
                size,
                collapsed: false,
            }
            .sized(size, SCREEN);
            let bar = place.bar(SCREEN);
            let layout = layout(bar, edge, false, texts(30));
            assert_eq!((layout.start.x, layout.start.y), (bar.x, bar.y), "{edge:?}");
            assert!(!layout.buttons.is_empty(), "{edge:?}");
            let mut rects = vec![layout.start];
            rects.extend(layout.buttons.iter().map(|b| b.rect));
            rects.extend(layout.hint);
            for (i, a) in rects.iter().enumerate() {
                assert!(inside(*a, bar), "{edge:?} {a:?}");
                for b in &rects[i + 1..] {
                    assert_eq!(a.intersection(*b).area(), 0, "{edge:?} {a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn horizontal_buttons_wrap_and_vertical_ones_stack() {
        let one = layout(Rect::new(0, 35, 120, 1), Edge::Bottom, false, texts(30));
        assert!(one.buttons.iter().all(|b| b.rect.y == 35));
        let three = layout(Rect::new(0, 33, 120, 3), Edge::Bottom, false, texts(30));
        assert!(three.buttons.len() > one.buttons.len());
        assert_eq!(three.buttons.last().map(|b| b.rect.y), Some(35));
        let side = layout(Rect::new(0, 0, 16, 36), Edge::Left, false, texts(5));
        let rows: Vec<u16> = side.buttons.iter().map(|b| b.rect.y).collect();
        assert_eq!(rows, [1, 2, 3, 4, 5]);
        assert!(side.buttons.iter().all(|b| b.rect.width <= 16));
        assert_eq!(side.hint.map(|h| h.y), Some(35));
    }

    #[test]
    fn sizes_stay_in_range_and_leave_desk_space() {
        let bottom = Place::default();
        assert_eq!(bottom.sized(9, SCREEN).size, ROWS.1);
        assert_eq!(bottom.sized(0, SCREEN).size, ROWS.0);
        assert_eq!(bottom.resized((5, 33), SCREEN).size, 3);
        let left = bottom.moved(Edge::Left, SCREEN);
        assert_eq!(left.resized((19, 4), SCREEN).size, 20);
        assert_eq!(left.resized((2, 4), SCREEN).size, COLUMNS.0);
        let tiny = Rect::new(0, 0, 30, 8);
        // 30 columns leave room for less than the minimum: the minimum wins.
        assert_eq!(left.sized(40, tiny).size, COLUMNS.0);
        assert_eq!(left.sized(40, Rect::new(0, 0, 50, 8)).size, 30);
        // Same orientation keeps the size.
        let top = Place {
            edge: Edge::Bottom,
            size: 3,
            collapsed: false,
        }
        .moved(Edge::Top, SCREEN);
        assert_eq!(top.size, 3);
    }

    #[test]
    fn the_nearest_edge_wins_relative_to_the_screen() {
        assert_eq!(nearest_edge((60, 1), SCREEN), Edge::Top);
        assert_eq!(nearest_edge((60, 34), SCREEN), Edge::Bottom);
        assert_eq!(nearest_edge((3, 18), SCREEN), Edge::Left);
        assert_eq!(nearest_edge((117, 18), SCREEN), Edge::Right);
        // 10 columns from the left of 120 is nearer than 5 rows from the top of 36.
        assert_eq!(nearest_edge((10, 5), SCREEN), Edge::Left);
    }

    #[test]
    fn a_collapsed_side_bar_is_narrow_with_a_toggle_and_keeps_its_size() {
        let place = Place {
            edge: Edge::Left,
            size: 24,
            collapsed: true,
        };
        assert_eq!(place.bar(SCREEN).width, COLLAPSED);
        assert_eq!(place.desk(SCREEN).x, COLLAPSED);
        assert!(place.grip(SCREEN).is_empty());
        let bar = place.bar(SCREEN);
        let side = layout(bar, Edge::Left, true, vec![task_short("Local", 4)]);
        assert_eq!(side.buttons[0].text, " L4");
        assert_eq!(side.hint, None);
        assert_eq!(side.toggle.map(|t| (t.x, t.y)), Some((2, bar.bottom() - 1)));
        let open = Place {
            collapsed: false,
            ..place
        };
        assert_eq!(open.bar(SCREEN).width, 24);
        let wide = layout(open.bar(SCREEN), Edge::Left, false, texts(2));
        assert_eq!(wide.toggle.map(|t| t.x), Some(22));
        // Top and bottom bars ignore the flag.
        let bottom = Place {
            collapsed: true,
            ..Place::default()
        };
        assert!(!bottom.is_collapsed());
        assert_eq!(bottom.bar(SCREEN).height, ROWS.0);
    }
}
