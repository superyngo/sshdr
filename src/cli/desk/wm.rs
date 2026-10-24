//! Window manager: overlapping windows in z-order, moved and resized against a hidden snap
//! grid, with edge snapping. Pure data and geometry, ported from wens-tui-lab `desk.rs`.

use ratatui::layout::{Position, Rect};

pub(super) type WinId = u32;

pub(super) const MIN_W: u16 = 16;
pub(super) const MIN_H: u16 = 5;
/// Hidden grid: the desk is divided into this many columns / rows; edges snap to the lines.
const GRID_COLS: u16 = 12;
const GRID_ROWS: u16 = 6;
/// Distance in cells within which an edge snaps to a grid line.
const SNAP: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WinState {
    Normal,
    Maximized { restore: Rect },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Window {
    pub id: WinId,
    pub title: String,
    pub rect: Rect,
    pub state: WinState,
}

impl Window {
    /// Content area inside the 1-cell frame.
    pub fn inner(&self) -> Rect {
        inner(self.rect)
    }
}

pub(super) fn inner(frame: Rect) -> Rect {
    Rect::new(
        frame.x.saturating_add(1),
        frame.y.saturating_add(1),
        frame.width.saturating_sub(2),
        frame.height.saturating_sub(2),
    )
}

/// Which borders a resize handle moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Desk {
    /// Z-order: index 0 is the bottom, the last window is on top.
    pub windows: Vec<Window>,
    pub focus: Option<WinId>,
    next_id: WinId,
}

impl Desk {
    pub fn get(&self, id: WinId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: WinId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Bring to the top of the z-order and focus.
    pub fn raise(&mut self, id: WinId) {
        if let Some(i) = self.windows.iter().position(|w| w.id == id) {
            let w = self.windows.remove(i);
            self.windows.push(w);
            self.focus = Some(id);
        }
    }

    pub fn toggle_maximize(&mut self, id: WinId, desk: Rect) {
        if let Some(w) = self.get_mut(id) {
            match w.state {
                WinState::Maximized { restore } => {
                    w.state = WinState::Normal;
                    w.rect = fit(restore, desk);
                }
                WinState::Normal => {
                    w.state = WinState::Maximized { restore: w.rect };
                    w.rect = desk;
                }
            }
        }
        self.raise(id);
    }

    pub fn close(&mut self, id: WinId) {
        self.windows.retain(|w| w.id != id);
        if self.focus == Some(id) {
            self.focus = self.windows.last().map(|w| w.id);
        }
    }

    /// Open a window at a cascading position with the wanted size, clamped into the desk.
    pub fn open(&mut self, title: String, size: (u16, u16), desk: Rect) -> WinId {
        self.next_id += 1;
        let id = self.next_id;
        let n = self.windows.len() as u16;
        let want = Rect::new(
            desk.x + 2 + 3 * (n % 6),
            desk.y + 1 + 2 * (n % 6),
            size.0,
            size.1,
        );
        self.windows.push(Window {
            id,
            title,
            rect: fit(want, desk),
            state: WinState::Normal,
        });
        self.focus = Some(id);
        id
    }

    /// Arrange all windows in a grid.
    pub fn tile_all(&mut self, desk: Rect) {
        let n = self.windows.len() as u16;
        if n == 0 {
            return;
        }
        let cols = (1..=n).find(|c| c * c >= n).unwrap_or(1);
        let rows = n.div_ceil(cols);
        for (i, w) in self.windows.iter_mut().enumerate() {
            let (c, r) = (i as u16 % cols, i as u16 / cols);
            let x0 = desk.x + desk.width * c / cols;
            let x1 = desk.x + desk.width * (c + 1) / cols;
            let y0 = desk.y + desk.height * r / rows;
            let y1 = desk.y + desk.height * (r + 1) / rows;
            w.state = WinState::Normal;
            w.rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
        }
    }

    /// Keep every window inside a (possibly shrunk) desk; maximized windows follow it.
    pub fn refit(&mut self, desk: Rect) {
        for w in &mut self.windows {
            w.rect = match w.state {
                WinState::Maximized { .. } => desk,
                WinState::Normal => fit(w.rect, desk),
            };
        }
    }
}

/// Keep a window inside the desk: shrink to the desk, enforce minimums when they fit, shift in.
pub(super) fn fit(want: Rect, desk: Rect) -> Rect {
    let w = want.width.max(MIN_W.min(desk.width));
    let h = want.height.max(MIN_H.min(desk.height));
    clamp_into(Rect::new(want.x, want.y, w, h), desk)
}

fn clamp_into(want: Rect, screen: Rect) -> Rect {
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

fn grid_lines(origin: u16, extent: u16, divisions: u16) -> Vec<u16> {
    (0..=divisions)
        .map(|i| origin + (u32::from(extent) * u32::from(i) / u32::from(divisions)) as u16)
        .collect()
}

fn snap(v: u16, lines: &[u16]) -> u16 {
    lines
        .iter()
        .copied()
        .filter(|l| l.abs_diff(v) <= SNAP)
        .min_by_key(|l| l.abs_diff(v))
        .unwrap_or(v)
}

/// Move: the grab offset keeps the cell under the pointer fixed. The left edge snaps to a grid
/// line; if it doesn't, the right edge may.
pub(super) fn moved(original: Rect, grab: (u16, u16), pointer: (u16, u16), desk: Rect) -> Rect {
    let x = pointer.0.saturating_sub(grab.0);
    let y = pointer.1.saturating_sub(grab.1);
    let xs = grid_lines(desk.x, desk.width, GRID_COLS);
    let ys = grid_lines(desk.y, desk.height, GRID_ROWS);
    let sx = snap(x, &xs);
    let x = if sx != x {
        sx
    } else {
        snap(x + original.width, &xs).saturating_sub(original.width)
    };
    let sy = snap(y, &ys);
    let y = if sy != y {
        sy
    } else {
        snap(y + original.height, &ys).saturating_sub(original.height)
    };
    clamp_into(Rect::new(x, y, original.width, original.height), desk)
}

/// Resize: the moving edges follow the pointer, snap to the grid, and stop at the minimum size
/// and at the desk border. Opposite edges stay fixed.
pub(super) fn resized(original: Rect, edges: Edges, pointer: (u16, u16), desk: Rect) -> Rect {
    let xs = grid_lines(desk.x, desk.width, GRID_COLS);
    let ys = grid_lines(desk.y, desk.height, GRID_ROWS);
    let (mut l, mut t, mut r, mut b) =
        (original.x, original.y, original.right(), original.bottom());
    if edges.left {
        l = snap(pointer.0, &xs).clamp(desk.x, r.saturating_sub(MIN_W).max(desk.x));
    }
    if edges.right {
        r = snap(pointer.0 + 1, &xs).clamp((l + MIN_W).min(desk.right()), desk.right());
    }
    if edges.top {
        t = snap(pointer.1, &ys).clamp(desk.y, b.saturating_sub(MIN_H).max(desk.y));
    }
    if edges.bottom {
        b = snap(pointer.1 + 1, &ys).clamp((t + MIN_H).min(desk.bottom()), desk.bottom());
    }
    Rect::new(l, t, r.saturating_sub(l), b.saturating_sub(t))
}

/// Edge snapping while moving: pointer on the left/right desk edge → that half;
/// on the top edge → the whole desk.
pub(super) fn snap_zone(pointer: (u16, u16), desk: Rect) -> Option<Rect> {
    let half = desk.width / 2;
    if pointer.1 == desk.y {
        Some(desk)
    } else if pointer.0 == desk.x {
        Some(Rect::new(desk.x, desk.y, half, desk.height))
    } else if pointer.0 + 1 >= desk.right() {
        Some(Rect::new(
            desk.x + half,
            desk.y,
            desk.width - half,
            desk.height,
        ))
    } else {
        None
    }
}

/// Resize handles of a window frame: left/right columns, bottom row, and the four corners.
/// The top row is the title bar (move), except its two corner cells.
pub(super) fn handles(frame: Rect) -> Vec<(Rect, Edges)> {
    if frame.width < 3 || frame.height < 3 {
        return Vec::new();
    }
    let (x, y, r, b) = (frame.x, frame.y, frame.right() - 1, frame.bottom() - 1);
    let h = frame.height - 2;
    let e = |left, right, top, bottom| Edges {
        left,
        right,
        top,
        bottom,
    };
    vec![
        (Rect::new(x, y + 1, 1, h), e(true, false, false, false)),
        (Rect::new(r, y + 1, 1, h), e(false, true, false, false)),
        (
            Rect::new(x + 1, b, frame.width - 2, 1),
            e(false, false, false, true),
        ),
        (Rect::new(x, y, 1, 1), e(true, false, true, false)),
        (Rect::new(r, y, 1, 1), e(false, true, true, false)),
        (Rect::new(x, b, 1, 1), e(true, false, false, true)),
        (Rect::new(r, b, 1, 1), e(false, true, false, true)),
    ]
}

/// Frame hit map: render records rect → target, input resolves clicks against the frame the
/// user actually saw. Last pushed wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HitMap<T> {
    entries: Vec<(Rect, T)>,
}

impl<T> Default for HitMap<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: Clone> HitMap<T> {
    pub fn push(&mut self, rect: Rect, target: T) {
        if !rect.is_empty() {
            self.entries.push((rect, target));
        }
    }

    pub fn hit(&self, column: u16, row: u16) -> Option<T> {
        let point = Position::new(column, row);
        self.entries
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(point))
            .map(|(_, target)| target.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESK: Rect = Rect {
        x: 0,
        y: 0,
        width: 96,
        height: 30,
    };

    fn desk_with(n: usize) -> Desk {
        let mut d = Desk::default();
        for i in 0..n {
            d.open(format!("w{i}"), (30, 10), DESK);
        }
        d
    }

    #[test]
    fn raise_moves_to_top_and_close_refocuses() {
        let mut d = desk_with(3);
        d.raise(1);
        assert_eq!(d.windows.last().map(|w| w.id), Some(1));
        assert_eq!(d.focus, Some(1));
        d.close(1);
        assert_eq!(d.focus, Some(3));
    }

    #[test]
    fn maximize_round_trips_and_refit_follows_desk() {
        let mut d = desk_with(1);
        let before = d.get(1).map(|w| w.rect);
        d.toggle_maximize(1, DESK);
        assert_eq!(d.get(1).map(|w| w.rect), Some(DESK));
        let small = Rect::new(0, 0, 40, 12);
        d.refit(small);
        assert_eq!(d.get(1).map(|w| w.rect), Some(small));
        d.toggle_maximize(1, DESK);
        assert_eq!(d.get(1).map(|w| w.rect), before);
    }

    #[test]
    fn move_snaps_left_edge_to_grid_and_stays_inside() {
        let w = Rect::new(10, 5, 20, 8);
        assert_eq!(moved(w, (0, 0), (17, 5), DESK).x, 16);
        assert_eq!(moved(w, (0, 0), (45, 5), DESK).x, 44);
        let far = moved(w, (0, 0), (500, 500), DESK);
        assert_eq!((far.right(), far.bottom()), (DESK.right(), DESK.bottom()));
    }

    #[test]
    fn resize_respects_minimum_and_desk() {
        let w = Rect::new(10, 5, 20, 8);
        let right = Edges {
            right: true,
            ..Edges::default()
        };
        assert_eq!(resized(w, right, (0, 5), DESK).width, MIN_W);
        assert_eq!(resized(w, right, (200, 5), DESK).right(), DESK.right());
        let tl = Edges {
            left: true,
            top: true,
            ..Edges::default()
        };
        let r = resized(w, tl, (31, 12), DESK);
        assert_eq!((r.right(), r.bottom()), (w.right(), w.bottom()));
        assert_eq!((r.width, r.height), (MIN_W, MIN_H));
    }

    #[test]
    fn tiny_desk_never_panics() {
        for desk in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 3, 2),
            Rect::new(0, 0, 10, 4),
        ] {
            let w = fit(Rect::new(5, 5, 40, 20), desk);
            assert!(w.right() <= desk.right() && w.bottom() <= desk.bottom());
            let _ = moved(w, (1, 1), (9, 9), desk);
            let _ = handles(w);
            let mut d = desk_with(3);
            d.tile_all(desk);
            d.refit(desk);
        }
    }

    #[test]
    fn edge_snap_zones() {
        assert_eq!(snap_zone((0, 10), DESK), Some(Rect::new(0, 0, 48, 30)));
        assert_eq!(snap_zone((95, 10), DESK), Some(Rect::new(48, 0, 48, 30)));
        assert_eq!(snap_zone((40, 0), DESK), Some(DESK));
        assert_eq!(snap_zone((40, 10), DESK), None);
    }

    #[test]
    fn tile_all_covers_desk_without_overlap() {
        let mut d = desk_with(3);
        d.tile_all(DESK);
        let area: u32 = d
            .windows
            .iter()
            .map(|w| u32::from(w.rect.width) * u32::from(w.rect.height))
            .sum();
        assert!(area <= u32::from(DESK.width) * u32::from(DESK.height));
        for (i, a) in d.windows.iter().enumerate() {
            for b in &d.windows[i + 1..] {
                assert!(!a.rect.intersects(b.rect));
            }
        }
    }

    #[test]
    fn hitmap_last_pushed_wins() {
        let mut map = HitMap::default();
        map.push(Rect::new(0, 0, 10, 10), 1);
        map.push(Rect::new(5, 5, 10, 10), 2);
        assert_eq!(map.hit(6, 6), Some(2));
        assert_eq!(map.hit(1, 1), Some(1));
        assert_eq!(map.hit(20, 20), None);
    }
}
