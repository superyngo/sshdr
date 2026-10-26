//! Overflow scrolling in **Native Windows** (B32): wheel and scrollbar drags for **Files** lists
//! and menus. Terminal Windows keep herdr's own scrollback.

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

use super::{App, Hit};
use crate::cli::desk::scroll::{Axis, BarDrag};
use crate::cli::desk::wm::WinId;

/// Rows or columns per wheel notch.
const STEP: isize = 3;

impl App {
    /// Wheel: scrolls a menu or **Files** window, else the pane below (herdr's scrollback).
    pub(super) fn wheel(&mut self, mouse: MouseEvent) {
        let back = matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollLeft
        );
        let delta = if back { -STEP } else { STEP };
        let horizontal = matches!(
            mouse.kind,
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight
        );
        let sideways = horizontal || mouse.modifiers.contains(KeyModifiers::SHIFT);
        match self.hits.hit(mouse.column, mouse.row) {
            Some(Hit::Menu(_)) => {
                let screen = self.desk_rect(); // menus draw inside the desk area
                if let Some(menu) = self.menu.as_mut() {
                    menu.scroll_by(delta, screen);
                }
            }
            Some(Hit::Files(id, _)) => {
                let rows = self.machine_rows();
                if let Some(f) = self.files.get_mut(&id) {
                    if sideways {
                        f.hscroll_by(delta);
                    } else {
                        f.scroll_by(delta, &rows);
                    }
                }
            }
            // Terminal Windows: herdr's scrollback, up and down only, as before B32.
            Some(Hit::Content(id, pane)) if !horizontal => {
                let (column, row) = (mouse.column, mouse.row);
                if let Some((x, y)) = self.content_point(id, pane, column, row) {
                    if let Some(session) = self.session(id, pane) {
                        session.scroll(back, x, y);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn files_bar_press(&mut self, win: WinId, axis: Axis, at: (u16, u16)) {
        let Some(f) = self.files.get_mut(&win) else {
            return;
        };
        let Some((track, extent)) = f.bars[usize::from(axis == Axis::X)] else {
            return;
        };
        let (drag, offset) = BarDrag::press(axis, track, extent, at);
        set(f, axis, offset);
        self.scroll_drag = Some((Some(win), drag));
    }

    pub(super) fn menu_bar_press(&mut self, at: (u16, u16)) {
        let screen = self.desk_rect(); // menus draw inside the desk area
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let Some(track) = menu.bar(screen) else {
            return;
        };
        let (drag, offset) = BarDrag::press(Axis::Y, track, menu.extent(screen), at);
        menu.scroll_to(offset, screen);
        self.scroll_drag = Some((None, drag));
    }

    pub(super) fn scroll_drag_to(&mut self, at: (u16, u16)) {
        let Some((target, drag)) = self.scroll_drag else {
            return;
        };
        let offset = drag.to(at);
        match target {
            Some(win) => {
                if let Some(f) = self.files.get_mut(&win) {
                    set(f, drag.axis, offset);
                }
            }
            None => {
                let screen = self.desk_rect(); // menus draw inside the desk area
                if let Some(menu) = self.menu.as_mut() {
                    menu.scroll_to(offset, screen);
                }
            }
        }
    }
}

fn set(f: &mut crate::cli::desk::files::Files, axis: Axis, offset: usize) {
    match axis {
        Axis::Y => f.scroll = offset,
        Axis::X => f.hscroll = offset,
    }
}
