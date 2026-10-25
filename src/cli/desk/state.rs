//! `desk.json` in the Session data directory: window geometry, maximize, minimize, and z-order,
//! keyed by terminal id. Client presentation state only (ADR 0007).

use std::path::{Path, PathBuf};

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use super::wm::{Desk, WinId, WinState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl From<Rect> for SavedRect {
    fn from(r: Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl From<SavedRect> for Rect {
    fn from(r: SavedRect) -> Self {
        Rect::new(r.x, r.y, r.width, r.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedWindow {
    pub terminal_id: String,
    pub rect: SavedRect,
    /// Size to restore to when the window was maximized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<SavedRect>,
    #[serde(default)]
    pub minimized: bool,
}

/// Windows bottom to top.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DeskState {
    pub windows: Vec<SavedWindow>,
}

pub(super) fn path() -> PathBuf {
    crate::session::data_dir().join("desk.json")
}

/// A missing or unreadable file is an empty state.
pub(super) fn load(path: &Path) -> DeskState {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub(super) fn save(path: &Path, state: &DeskState) {
    let result = serde_json::to_vec_pretty(state)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(path, bytes));
    if let Err(err) = result {
        tracing::warn!(%err, path = %path.display(), "desk: saving window state failed");
    }
}

/// Snapshot `desk` in z-order; `terminal` maps a window to its terminal id.
pub(super) fn capture(desk: &Desk, terminal: impl Fn(WinId) -> Option<String>) -> DeskState {
    DeskState {
        windows: desk
            .windows
            .iter()
            .filter_map(|w| {
                Some(SavedWindow {
                    terminal_id: terminal(w.id)?,
                    rect: w.rect.into(),
                    restore: match w.state {
                        WinState::Maximized { restore } => Some(restore.into()),
                        WinState::Normal => None,
                    },
                    minimized: w.minimized,
                })
            })
            .collect(),
    }
}

/// Apply saved geometry and flags to open windows, then restack them in the saved order;
/// windows without a saved entry stay on top in their current order.
pub(super) fn restore(
    desk: &mut Desk,
    state: &DeskState,
    terminal: impl Fn(WinId) -> Option<String>,
    area: Rect,
) {
    let rank = |id: WinId| {
        terminal(id).and_then(|t| state.windows.iter().position(|s| s.terminal_id == t))
    };
    for w in &mut desk.windows {
        let Some(saved) = rank(w.id).map(|i| &state.windows[i]) else {
            continue;
        };
        w.minimized = saved.minimized;
        match saved.restore {
            Some(restore) => {
                w.state = WinState::Maximized {
                    restore: super::wm::fit(restore.into(), area),
                };
                w.rect = area;
            }
            None => {
                w.state = WinState::Normal;
                w.rect = super::wm::fit(saved.rect.into(), area);
            }
        }
    }
    desk.windows
        .sort_by_key(|w| rank(w.id).map_or((1, 0), |i| (0, i)));
    desk.focus = desk.top_visible();
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 30,
    };

    fn desk(n: usize) -> Desk {
        let mut d = Desk::default();
        for i in 0..n {
            d.open(format!("w{i}"), (30, 10), AREA);
        }
        d
    }

    fn term(id: WinId) -> Option<String> {
        Some(format!("t{id}"))
    }

    #[test]
    fn round_trip_keeps_geometry_flags_and_order() {
        let mut a = desk(3);
        let (one, two, three) = (a.windows[0].id, a.windows[1].id, a.windows[2].id);
        a.get_mut(one).unwrap().rect = Rect::new(40, 5, 50, 20);
        a.toggle_maximize(two, AREA);
        a.minimize(three);
        a.raise(one);
        let saved = capture(&a, term);
        let json = serde_json::to_string(&saved).unwrap();
        let loaded: DeskState = serde_json::from_str(&json).unwrap();

        let mut b = desk(3);
        restore(&mut b, &loaded, term, AREA);
        assert_eq!(
            b.windows.iter().map(|w| w.id).collect::<Vec<_>>(),
            a.windows.iter().map(|w| w.id).collect::<Vec<_>>()
        );
        assert_eq!(b.get(one).unwrap().rect, Rect::new(40, 5, 50, 20));
        assert_eq!(b.get(two).unwrap().rect, AREA);
        assert!(matches!(
            b.get(two).unwrap().state,
            WinState::Maximized { .. }
        ));
        assert!(b.get(three).unwrap().minimized);
        assert_eq!(b.focus, Some(one));
    }

    #[test]
    fn unknown_windows_stay_on_top_and_bad_files_load_empty() {
        let mut d = desk(2);
        let state = DeskState {
            windows: vec![SavedWindow {
                terminal_id: "t1".into(),
                rect: Rect::new(500, 500, 10, 10).into(),
                restore: None,
                minimized: false,
            }],
        };
        restore(&mut d, &state, term, AREA);
        assert_eq!(d.windows[1].id, 2);
        let r = d.get(1).unwrap().rect;
        assert!(r.right() <= AREA.right() && r.bottom() <= AREA.bottom());

        let dir = std::env::temp_dir().join(format!("desk-state-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("desk.json");
        std::fs::write(&file, b"not json").unwrap();
        assert_eq!(load(&file), DeskState::default());
        save(&file, &state);
        assert_eq!(load(&file), state);
    }
}
