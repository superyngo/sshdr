//! Overflow scrolling in **Native Windows** (B32): wheel and scrollbar drags for **Files** lists
//! and menus. Terminal Windows keep herdr's own scrollback.

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

use super::{App, Hit};
use crate::cli::desk::scroll::{Axis, BarDrag};
use crate::cli::desk::wm::WinId;

/// A drag inside a **Native Window** or menu, until the button is released.
#[derive(Debug, Clone, Copy)]
pub(super) enum NativeDrag {
    /// A scrollbar thumb: of a **Files** window, or the menu (`None`).
    Bar(Option<WinId>, BarDrag),
    /// A selection box in a **Files** window (F3a).
    Box(WinId),
    /// Selected **Files** items, pressed at `from`, now at `at` (F3c).
    Items {
        win: WinId,
        from: (u16, u16),
        at: (u16, u16),
    },
}

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
        self.scroll_drag = Some(NativeDrag::Bar(Some(win), drag));
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
        self.scroll_drag = Some(NativeDrag::Bar(None, drag));
    }

    pub(super) fn scroll_drag_to(&mut self, at: (u16, u16)) {
        let (target, drag) = match self.scroll_drag {
            Some(NativeDrag::Bar(target, drag)) => (target, drag),
            Some(NativeDrag::Box(win)) => {
                if let Some(f) = self.files.get_mut(&win) {
                    f.box_to(at);
                }
                return;
            }
            Some(NativeDrag::Items { win, from, .. }) => {
                self.scroll_drag = Some(NativeDrag::Items { win, from, at });
                return;
            }
            None => return,
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

impl App {
    /// Button released: any bar, box, or item drag ends; dragged items drop.
    pub(super) fn native_up(&mut self, mouse: MouseEvent) {
        match self.scroll_drag.take() {
            Some(NativeDrag::Box(win)) => {
                if let Some(f) = self.files.get_mut(&win) {
                    f.box_end();
                }
            }
            Some(NativeDrag::Items { win, from, at }) if from != at => self.files_drop(win, mouse),
            _ => {}
        }
    }
}

fn set(f: &mut crate::cli::desk::files::Files, axis: Axis, offset: usize) {
    match axis {
        Axis::Y => f.scroll = offset,
        Axis::X => f.hscroll = offset,
    }
}
