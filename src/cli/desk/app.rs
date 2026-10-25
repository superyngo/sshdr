//! Desk event loop: raw host input, window drag sessions, per-window session streams, and
//! composition of the mirrored terminals into one frame.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Clear, Widget},
    Frame, Terminal,
};

use super::dialog::{Dialog, DialogButton, Outcome};
use super::menu::{Menu, MenuHit, MenuItem};
use super::server::{self, Snapshot, TaskInfo, WindowInfo};
use super::stream::{DeskEvent, Session};
use super::taskbar;
use super::wm::{self, Desk, Edges, HitMap, WinId, WinState};
use crate::pane::desk_mirror::DeskMirror as Mirror;
use crate::raw_input::{parse_sgr_mouse_report, RawInputByteFramer};

/// Desk command prefix (Ctrl+\); the next key is a desk command.
const PREFIX: u8 = 0x1c;
const MIN_DRAW_INTERVAL: Duration = Duration::from_millis(16);
const INPUT_FLUSH: Duration = Duration::from_millis(30);
const IDLE_TICK: Duration = Duration::from_millis(250);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const NEW_WINDOW_SIZE: (u16, u16) = (84, 26);

const DESK_BG: Color = Color::Indexed(24);
const BAR_BG: Color = Color::Indexed(236);
const FOCUS_BORDER: Color = Color::Indexed(81);
const BLUR_BORDER: Color = Color::Indexed(245);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Desktop,
    Content(WinId),
    Title(WinId),
    Handle(WinId, Edges),
    Minimize(WinId),
    Maximize(WinId),
    Close(WinId),
    Start,
    Task(usize),
    Menu(MenuHit),
    Dialog(DialogButton),
}

/// What a menu row does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Raise(WinId),
    Minimize(WinId),
    ToggleMaximize(WinId),
    Close(WinId),
    RenameWindow(WinId),
    NewWindowIn(String),
    NewWindow,
    NewTask,
    TileAll,
    Quit,
    RenameTask(String),
    MinimizeTask(String),
    RestoreTask(String),
    CloseTask(String),
}

/// What a dialog's primary button does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DialogTarget {
    RenameWindow(String),
    RenameTask(String),
    CloseTask(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move { grab: (u16, u16) },
    Resize { edges: Edges },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: WinId,
    kind: DragKind,
    original: Rect,
    desk: Rect,
    zone: Option<Rect>,
}

struct WinData {
    info: WindowInfo,
    attached_at: Instant,
    session: Session,
    mirror: Mirror,
    sent_size: (u16, u16),
}

#[derive(Default)]
struct Stats {
    frames: u64,
    frame_bytes: u64,
    draws: u64,
    draw_total: Duration,
    draw_max: Duration,
}

struct App {
    tasks: Vec<TaskInfo>,
    events: Sender<DeskEvent>,
    desk: Desk,
    data: HashMap<WinId, WinData>,
    screen: Rect,
    hits: HitMap<Hit>,
    drag: Option<Drag>,
    content_press: Option<WinId>,
    last_title_click: Option<(WinId, Instant)>,
    prefix_armed: bool,
    menu: Option<Menu<Action>>,
    dialog: Option<Dialog<DialogTarget>>,
    quit: bool,
    notice: Option<String>,
    stats: Stats,
}

fn desk_area(screen: Rect) -> Rect {
    Rect::new(
        screen.x,
        screen.y,
        screen.width,
        screen.height.saturating_sub(1),
    )
}

impl App {
    fn desk_rect(&self) -> Rect {
        desk_area(self.screen)
    }

    fn attach(&mut self, info: WindowInfo, rect: Option<Rect>) -> io::Result<WinId> {
        let desk = self.desk_rect();
        let id = self.desk.open(window_title(&info), NEW_WINDOW_SIZE, desk);
        if let (Some(rect), Some(w)) = (rect, self.desk.get_mut(id)) {
            w.rect = wm::fit(rect, desk);
        }
        let inner = self.desk.get(id).map(|w| w.inner()).unwrap_or_default();
        let size = (inner.width.max(1), inner.height.max(1));
        let started = Mirror::new(size.0, size.1).and_then(|mirror| {
            Session::spawn(id, &info.terminal_id, size.0, size.1, self.events.clone())
                .map(|session| (mirror, session))
        });
        match started {
            Ok((mirror, session)) => {
                self.data.insert(
                    id,
                    WinData {
                        info,
                        attached_at: Instant::now(),
                        session,
                        mirror,
                        sent_size: size,
                    },
                );
                Ok(id)
            }
            Err(err) => {
                self.desk.close(id);
                Err(err)
            }
        }
    }

    fn win_id(&self, tab_id: &str) -> Option<WinId> {
        self.data
            .iter()
            .find(|(_, data)| data.info.tab_id == tab_id)
            .map(|(id, _)| *id)
    }

    /// The focused window's **Task**, else the first one.
    fn current_task(&self) -> Option<String> {
        self.desk
            .focus
            .and_then(|id| self.data.get(&id))
            .map(|data| data.info.workspace_id.clone())
            .or_else(|| self.tasks.first().map(|t| t.workspace_id.clone()))
    }

    /// New window in the current **Task**; creates a **Task** when there is none.
    fn new_window(&mut self) {
        let created = match self.current_task() {
            Some(workspace_id) => server::create_window(&workspace_id),
            None => server::create_task().map(|(task, window)| {
                self.tasks.push(task);
                window
            }),
        };
        if let Err(err) = created.and_then(|info| self.attach(info, None)) {
            self.notice = Some(format!("new window failed: {err}"));
        }
    }

    /// Reconcile windows with server state: new tabs open, closed tabs go, labels follow.
    fn apply(&mut self, snap: Snapshot) {
        self.tasks = snap.tasks;
        for info in snap.windows.iter().cloned() {
            match self.win_id(&info.tab_id) {
                Some(id) if self.data[&id].info.terminal_id == info.terminal_id => {
                    if let Some(w) = self.desk.get_mut(id) {
                        w.title = window_title(&info);
                    }
                    if let Some(data) = self.data.get_mut(&id) {
                        data.info = info;
                    }
                }
                found => {
                    let rect = found.and_then(|id| self.desk.get(id).map(|w| w.rect));
                    if let Some(id) = found {
                        self.drop_window(id);
                    }
                    if let Err(err) = self.attach(info, rect) {
                        self.notice = Some(format!("open window failed: {err}"));
                    }
                }
            }
        }
        // A snapshot taken before a window was created must not drop it.
        let grace = server::POLL_INTERVAL * 2;
        let gone: Vec<WinId> = self
            .data
            .iter()
            .filter(|(_, data)| {
                data.attached_at.elapsed() > grace
                    && !snap.windows.iter().any(|w| w.tab_id == data.info.tab_id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.drop_window(id);
        }
    }

    /// Remove a window from the desk; its tab stays on the server.
    fn drop_window(&mut self, id: WinId) {
        self.desk.close(id);
        if let Some(data) = self.data.remove(&id) {
            data.session.release();
        }
    }

    fn close_window(&mut self, id: WinId) {
        self.desk.close(id);
        if let Some(data) = self.data.remove(&id) {
            data.session.release();
            server::close_window(&data.info.tab_id);
        }
    }

    /// Send a resize for every window whose content size changed since the last one sent.
    fn sync_sizes(&mut self) {
        for w in &self.desk.windows {
            let inner = w.inner();
            let size = (inner.width, inner.height);
            if size.0 == 0 || size.1 == 0 {
                continue;
            }
            if let Some(data) = self.data.get_mut(&w.id) {
                if data.sent_size != size {
                    data.sent_size = size;
                    data.session.resize(size.0, size.1);
                }
            }
        }
    }

    fn handle_event(&mut self, event: DeskEvent, framer: &mut RawInputByteFramer) {
        match event {
            DeskEvent::Input(bytes) => {
                for chunk in framer.push(&bytes) {
                    self.handle_chunk(&chunk);
                }
            }
            DeskEvent::Frame {
                win,
                width,
                height,
                bytes,
            } => {
                self.stats.frames += 1;
                self.stats.frame_bytes += bytes.len() as u64;
                if let Some(data) = self.data.get_mut(&win) {
                    data.mirror.feed(width, height, &bytes);
                }
            }
            // The pane exited (or the stream ended): drop the window, keep the tab alone.
            DeskEvent::Closed(win) => self.drop_window(win),
            DeskEvent::Snapshot(snap) => self.apply(snap),
        }
    }

    fn handle_chunk(&mut self, chunk: &[u8]) {
        if let Some(report) = parse_sgr_mouse_report(chunk) {
            let (Some(x), Some(y)) = (report.x.checked_sub(1), report.y.checked_sub(1)) else {
                return;
            };
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                return;
            };
            self.handle_mouse(report.at_cell(x, y));
            return;
        }
        if let Some(dialog) = self.dialog.as_mut() {
            match dialog.key(chunk) {
                Outcome::Pending => {}
                Outcome::Submit => self.submit_dialog(),
                Outcome::Cancel => self.dialog = None,
            }
            return;
        }
        if let Some(menu) = self.menu.as_mut() {
            match chunk {
                b"\x1b[A" | b"\x1bOA" => menu.step(-1),
                b"\x1b[B" | b"\x1bOB" => menu.step(1),
                b"\r" | b"\n" => {
                    let action = menu.action(menu.highlighted);
                    self.menu = None;
                    if let Some(action) = action {
                        self.run_action(action);
                    }
                }
                b"\x1b" => self.menu = None,
                // Any other key closes the menu and is handled normally.
                _ => {
                    self.menu = None;
                    return self.handle_chunk(chunk);
                }
            }
            return;
        }
        if self.prefix_armed {
            self.prefix_armed = false;
            match chunk {
                b"n" => self.new_window(),
                b"w" => {
                    if let Some(id) = self.desk.focus {
                        self.close_window(id);
                    }
                }
                b"t" => {
                    self.desk.tile_all(self.desk_rect());
                    self.sync_sizes();
                }
                b"z" => {
                    if let Some(id) = self.desk.focus {
                        self.desk.minimize(id);
                    }
                }
                b"m" => {
                    if let Some(id) = self.desk.focus {
                        self.desk.toggle_maximize(id, self.desk_rect());
                        self.sync_sizes();
                    }
                }
                b"q" => self.quit = true,
                b"\t" => {
                    if let Some(first) = self.desk.windows.first().map(|w| w.id) {
                        self.desk.raise(first);
                    }
                }
                [PREFIX] => self.forward_input(chunk),
                _ => {}
            }
            return;
        }
        if chunk == [PREFIX] {
            self.prefix_armed = true;
            return;
        }
        self.forward_input(chunk);
    }

    fn forward_input(&mut self, bytes: &[u8]) {
        if let Some(data) = self.desk.focus.and_then(|id| self.data.get_mut(&id)) {
            data.session.input(bytes);
        }
    }

    fn content_point(&self, id: WinId, column: u16, row: u16) -> Option<(u16, u16)> {
        let inner = self.desk.get(id)?.inner();
        let x = column
            .checked_sub(inner.x)?
            .min(inner.width.saturating_sub(1));
        let y = row
            .checked_sub(inner.y)?
            .min(inner.height.saturating_sub(1));
        Some((x, y))
    }

    fn forward_mouse(&mut self, id: WinId, action: &str, button: &str, column: u16, row: u16) {
        let Some((x, y)) = self.content_point(id, column, row) else {
            return;
        };
        if let Some(data) = self.data.get_mut(&id) {
            data.session.mouse(action, button, x, y, 0);
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let (column, row) = (mouse.column, mouse.row);
        let desk = self.desk_rect();
        match mouse.kind {
            MouseEventKind::Down(button) => {
                let hit = self.hits.hit(column, row);
                if self.dialog.is_some() {
                    match hit {
                        Some(Hit::Dialog(DialogButton::Primary)) => self.submit_dialog(),
                        Some(Hit::Dialog(DialogButton::Cancel)) => self.dialog = None,
                        _ => {}
                    }
                    return;
                }
                if self.menu.is_some() {
                    match hit {
                        Some(Hit::Menu(MenuHit::Row(index))) => {
                            let action = self.menu.take().and_then(|m| m.action(index));
                            if let Some(action) = action {
                                self.run_action(action);
                            }
                        }
                        Some(Hit::Menu(MenuHit::Body)) => {}
                        _ => self.menu = None,
                    }
                    return;
                }
                if let Some(id) = hit_window(hit) {
                    self.desk.raise(id);
                }
                match (hit, button) {
                    (Some(Hit::Content(id)), button) => {
                        self.content_press = Some(id);
                        self.forward_mouse(id, "down", button_name(button), column, row);
                    }
                    (Some(Hit::Title(id)), MouseButton::Left) => {
                        let now = Instant::now();
                        let double = matches!(self.last_title_click,
                            Some((last, at)) if last == id && now.duration_since(at) < DOUBLE_CLICK);
                        self.last_title_click = Some((id, now));
                        if double {
                            self.desk.toggle_maximize(id, desk);
                            self.sync_sizes();
                            return;
                        }
                        if let Some(w) = self.desk.get(id) {
                            let original = w.rect;
                            self.drag = Some(Drag {
                                id,
                                kind: DragKind::Move {
                                    grab: (column - original.x, row - original.y),
                                },
                                original,
                                desk,
                                zone: None,
                            });
                        }
                    }
                    (Some(Hit::Handle(id, edges)), MouseButton::Left) => {
                        if let Some(w) = self.desk.get(id) {
                            self.drag = Some(Drag {
                                id,
                                kind: DragKind::Resize { edges },
                                original: w.rect,
                                desk,
                                zone: None,
                            });
                        }
                    }
                    (Some(Hit::Title(id)), MouseButton::Right) => {
                        self.menu = self.window_menu(id, (column, row));
                    }
                    (Some(Hit::Minimize(id)), MouseButton::Left) => self.desk.minimize(id),
                    (Some(Hit::Maximize(id)), MouseButton::Left) => {
                        self.desk.toggle_maximize(id, desk);
                        self.sync_sizes();
                    }
                    (Some(Hit::Close(id)), MouseButton::Left) => self.close_window(id),
                    (Some(Hit::Start), MouseButton::Left) => {
                        self.menu = Some(start_menu((self.screen.x, row)));
                    }
                    (Some(Hit::Task(index)), button) => {
                        let at = (column, row);
                        self.menu = match button {
                            MouseButton::Right => self.task_menu(index, at),
                            _ => self.task_windows_menu(index, at),
                        };
                    }
                    _ => {}
                }
            }
            MouseEventKind::Drag(button) => {
                if let Some(mut drag) = self.drag {
                    let pointer = (column, row);
                    let rect = match drag.kind {
                        DragKind::Move { grab } => {
                            drag.zone = wm::snap_zone(pointer, drag.desk);
                            wm::moved(drag.original, grab, pointer, drag.desk)
                        }
                        DragKind::Resize { edges } => {
                            wm::resized(drag.original, edges, pointer, drag.desk)
                        }
                    };
                    if let Some(w) = self.desk.get_mut(drag.id) {
                        w.rect = rect;
                        w.state = WinState::Normal;
                    }
                    self.drag = Some(drag);
                } else if let Some(id) = self.content_press {
                    self.forward_mouse(id, "drag", button_name(button), column, row);
                }
            }
            MouseEventKind::Up(button) => {
                if let Some(drag) = self.drag.take() {
                    if let (Some(zone), Some(w)) = (drag.zone, self.desk.get_mut(drag.id)) {
                        w.rect = zone;
                        if zone == drag.desk {
                            w.state = WinState::Maximized {
                                restore: drag.original,
                            };
                        }
                    }
                    self.sync_sizes();
                } else if let Some(id) = self.content_press.take() {
                    self.forward_mouse(id, "up", button_name(button), column, row);
                }
            }
            MouseEventKind::Moved => {
                if let Some(Hit::Content(id)) = self.hits.hit(column, row) {
                    if Some(id) == self.desk.focus {
                        self.forward_mouse(id, "move", "left", column, row);
                    }
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if let Some(Hit::Content(id)) = self.hits.hit(column, row) {
                    let up = mouse.kind == MouseEventKind::ScrollUp;
                    if let Some((x, y)) = self.content_point(id, column, row) {
                        if let Some(data) = self.data.get_mut(&id) {
                            data.session.scroll(up, x, y);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let screen = frame.area();
        let desk = desk_area(screen);
        let mut hits = HitMap::default();
        frame
            .buffer_mut()
            .set_style(desk, Style::default().bg(DESK_BG));
        hits.push(desk, Hit::Desktop);

        for w in self.desk.windows.iter().filter(|w| !w.minimized) {
            let rect = w.rect.intersection(desk);
            if rect.width < 3 || rect.height < 3 {
                continue;
            }
            let focused = self.desk.focus == Some(w.id);
            let border = Style::default().fg(if focused { FOCUS_BORDER } else { BLUR_BORDER });
            Clear.render(rect, frame.buffer_mut());
            Block::bordered()
                .border_style(border)
                .render(rect, frame.buffer_mut());
            let inner = wm::inner(rect);
            if let Some(data) = self.data.get(&w.id) {
                data.mirror.render(frame, inner, focused);
            }
            // Title bar: title on the left, [_][□][×] on the right.
            let buttons_w = 9u16;
            let title_w = rect.width.saturating_sub(2 + buttons_w + 1);
            let title_style = if focused {
                border.add_modifier(Modifier::BOLD)
            } else {
                border
            };
            frame.buffer_mut().set_stringn(
                rect.x + 1,
                rect.y,
                format!(" {} ", w.title),
                usize::from(title_w),
                title_style,
            );
            hits.push(inner, Hit::Content(w.id));
            hits.push(
                Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
                Hit::Title(w.id),
            );
            if rect.width > buttons_w + 4 {
                let bx = rect.right() - 1 - buttons_w;
                frame.buffer_mut().set_string(bx, rect.y, "[_]", border);
                hits.push(Rect::new(bx, rect.y, 3, 1), Hit::Minimize(w.id));
                let bx = bx + 3;
                frame.buffer_mut().set_string(bx, rect.y, "[□]", border);
                frame.buffer_mut().set_string(
                    bx + 3,
                    rect.y,
                    "[×]",
                    Style::default().fg(Color::LightRed),
                );
                hits.push(Rect::new(bx, rect.y, 3, 1), Hit::Maximize(w.id));
                hits.push(Rect::new(bx + 3, rect.y, 3, 1), Hit::Close(w.id));
            }
            for (handle, edges) in wm::handles(rect) {
                hits.push(handle, Hit::Handle(w.id, edges));
            }
        }

        if let Some(zone) = self.drag.and_then(|drag| drag.zone) {
            Block::bordered()
                .border_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
                .render(zone, frame.buffer_mut());
        }

        // Bottom bar.
        let bar = Rect::new(screen.x, screen.bottom().saturating_sub(1), screen.width, 1);
        if bar.height == 1 && screen.height > 1 {
            let buf = frame.buffer_mut();
            buf.set_style(bar, Style::default().bg(BAR_BG).fg(Color::White));
            let mut x = bar.x;
            let start = " ⊞ Start ";
            let start_style = if self.menu.as_ref().is_some_and(|m| m.title == START_TITLE) {
                Style::default().bg(FOCUS_BORDER).fg(Color::Black)
            } else {
                Style::default()
                    .bg(Color::Indexed(25))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            };
            let width = super::text_width(start);
            buf.set_string(x, bar.y, start, start_style);
            hits.push(Rect::new(x, bar.y, width, 1), Hit::Start);
            x += width + 1;
            let current = self.current_task();
            let texts = self.tasks.iter().map(|task| {
                let count = self
                    .data
                    .values()
                    .filter(|data| data.info.workspace_id == task.workspace_id)
                    .count();
                taskbar::task_text(&task.label, count)
            });
            let buttons = taskbar::layout(x + 1, bar.right(), texts.collect::<Vec<_>>());
            for (index, button) in buttons.iter().enumerate() {
                let active = current.as_deref() == Some(self.tasks[index].workspace_id.as_str());
                let style = if active {
                    Style::default().bg(FOCUS_BORDER).fg(Color::Black)
                } else {
                    Style::default().bg(Color::Indexed(239)).fg(Color::White)
                };
                buf.set_string(button.x, bar.y, &button.text, style);
                hits.push(
                    Rect::new(button.x, bar.y, button.width, 1),
                    Hit::Task(index),
                );
            }
            if let Some(last) = buttons.last() {
                x = last.x + last.width;
            }
            let hint = match (&self.notice, self.prefix_armed) {
                (_, true) => {
                    "desk: n new · w close · z min · m max · t tile · Tab next · q quit".to_string()
                }
                (Some(notice), false) => notice.clone(),
                (None, false) => format!("{} windows · Ctrl+\\ desk keys", self.desk.windows.len()),
            };
            buf.set_stringn(
                x + 1,
                bar.y,
                hint,
                usize::from(bar.right().saturating_sub(x + 1)),
                Style::default().bg(BAR_BG).fg(Color::Gray),
            );
        }
        if let Some(menu) = &self.menu {
            menu.render(desk, frame.buffer_mut(), |rect, hit| {
                hits.push(rect, Hit::Menu(hit));
            });
        }
        if let Some(dialog) = &self.dialog {
            hits.push(screen, Hit::Desktop);
            dialog.render(screen, frame.buffer_mut(), |rect, button| {
                hits.push(rect, Hit::Dialog(button));
            });
        }
        self.hits = hits;
    }

    fn window_menu(&self, id: WinId, at: (u16, u16)) -> Option<Menu<Action>> {
        let w = self.desk.get(id)?;
        let maximize = match w.state {
            WinState::Maximized { .. } => "Restore size",
            WinState::Normal => "Maximize",
        };
        Menu::open(
            w.title.clone(),
            at,
            vec![
                MenuItem::new("Rename…", "", Action::RenameWindow(id)),
                MenuItem::new("Minimize", "^\\ z", Action::Minimize(id)),
                MenuItem::new(maximize, "^\\ m", Action::ToggleMaximize(id)),
                MenuItem::new("Close", "^\\ w", Action::Close(id)),
            ],
        )
    }

    /// Left click on a Task button: its windows, minimized ones marked.
    fn task_windows_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let task = self.tasks.get(index)?;
        let items = self
            .desk
            .windows
            .iter()
            .filter(|w| {
                self.data
                    .get(&w.id)
                    .is_some_and(|d| d.info.workspace_id == task.workspace_id)
            })
            .map(|w| {
                let mark = if w.minimized { "▁ " } else { "  " };
                MenuItem::new(format!("{mark}{}", w.title), "", Action::Raise(w.id))
            })
            .collect();
        Menu::open(task.label.clone(), at, items).map(Menu::above)
    }

    fn task_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let task = self.tasks.get(index)?;
        let ws = &task.workspace_id;
        Menu::open(
            task.label.clone(),
            at,
            vec![
                MenuItem::new("Rename…", "", Action::RenameTask(ws.clone())),
                MenuItem::new("New window", "", Action::NewWindowIn(ws.clone())),
                MenuItem::new("Minimize all", "", Action::MinimizeTask(ws.clone())),
                MenuItem::new("Restore all", "", Action::RestoreTask(ws.clone())),
                MenuItem::new("Close task…", "", Action::CloseTask(ws.clone())),
            ],
        )
        .map(Menu::above)
    }

    fn task_windows(&self, workspace_id: &str) -> Vec<WinId> {
        self.desk
            .windows
            .iter()
            .map(|w| w.id)
            .filter(|id| {
                self.data
                    .get(id)
                    .is_some_and(|d| d.info.workspace_id == workspace_id)
            })
            .collect()
    }

    fn task_label(&self, workspace_id: &str) -> String {
        self.tasks
            .iter()
            .find(|t| t.workspace_id == workspace_id)
            .map(|t| t.label.clone())
            .unwrap_or_default()
    }

    fn run_action(&mut self, action: Action) {
        let desk = self.desk_rect();
        match action {
            Action::Raise(id) => self.desk.raise(id),
            Action::Minimize(id) => self.desk.minimize(id),
            Action::ToggleMaximize(id) => {
                self.desk.toggle_maximize(id, desk);
                self.sync_sizes();
            }
            Action::Close(id) => self.close_window(id),
            Action::RenameWindow(id) => {
                if let Some(data) = self.data.get(&id) {
                    self.dialog = Some(Dialog::input(
                        "Rename window",
                        &data.info.label,
                        "Rename",
                        DialogTarget::RenameWindow(data.info.tab_id.clone()),
                    ));
                }
            }
            Action::NewWindow => self.new_window(),
            Action::NewTask => match server::create_task() {
                Ok((task, info)) => {
                    self.tasks.push(task);
                    if let Err(err) = self.attach(info, None) {
                        self.notice = Some(format!("new task failed: {err}"));
                    }
                }
                Err(err) => self.notice = Some(format!("new task failed: {err}")),
            },
            Action::TileAll => {
                self.desk.tile_all(desk);
                self.sync_sizes();
            }
            Action::Quit => self.quit = true,
            Action::NewWindowIn(ws) => {
                match server::create_window(&ws).and_then(|info| self.attach(info, None)) {
                    Ok(_) => {}
                    Err(err) => self.notice = Some(format!("new window failed: {err}")),
                }
            }
            Action::RenameTask(ws) => {
                let label = self.task_label(&ws);
                self.dialog = Some(Dialog::input(
                    "Rename task",
                    &label,
                    "Rename",
                    DialogTarget::RenameTask(ws),
                ));
            }
            Action::MinimizeTask(ws) => {
                for id in self.task_windows(&ws) {
                    self.desk.minimize(id);
                }
            }
            Action::RestoreTask(ws) => {
                for id in self.task_windows(&ws) {
                    self.desk.raise(id);
                }
            }
            Action::CloseTask(ws) => {
                let label = self.task_label(&ws);
                let count = self.task_windows(&ws).len();
                self.dialog = Some(Dialog::confirm(
                    "Close task",
                    format!("Close task {label} and its {count} windows?"),
                    "Close",
                    DialogTarget::CloseTask(ws),
                ));
            }
        }
    }

    fn submit_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let text = dialog.input.as_deref().map(str::trim).unwrap_or_default();
        match dialog.target {
            DialogTarget::RenameWindow(tab_id) if !text.is_empty() => {
                server::rename_window(&tab_id, text);
                if let Some(id) = self.win_id(&tab_id) {
                    let mut info = self.data[&id].info.clone();
                    info.label = text.to_string();
                    if let Some(w) = self.desk.get_mut(id) {
                        w.title = window_title(&info);
                    }
                    if let Some(data) = self.data.get_mut(&id) {
                        data.info = info;
                    }
                }
            }
            DialogTarget::RenameTask(ws) if !text.is_empty() => {
                server::rename_task(&ws, text);
                if let Some(task) = self.tasks.iter_mut().find(|t| t.workspace_id == ws) {
                    task.label = text.to_string();
                }
            }
            DialogTarget::CloseTask(ws) => {
                for id in self.task_windows(&ws) {
                    self.drop_window(id);
                }
                self.tasks.retain(|t| t.workspace_id != ws);
                server::close_task(&ws);
            }
            _ => {}
        }
    }
}

const START_TITLE: &str = "Start";

/// The **Start Menu**, opening upward from the Start button.
fn start_menu(at: (u16, u16)) -> Menu<Action> {
    let items = vec![
        MenuItem::new("New window", "^\\ n", Action::NewWindow),
        MenuItem::new("New task", "", Action::NewTask),
        MenuItem::new("Tile all", "^\\ t", Action::TileAll),
        MenuItem::new("Quit desk", "^\\ q", Action::Quit),
    ];
    Menu {
        title: START_TITLE.to_string(),
        at,
        above: true,
        items,
        highlighted: 0,
    }
}

fn window_title(info: &WindowInfo) -> String {
    match info.other_panes {
        0 => info.label.clone(),
        n => format!("{} (+{n} panes)", info.label),
    }
}

fn hit_window(hit: Option<Hit>) -> Option<WinId> {
    match hit? {
        Hit::Content(id)
        | Hit::Title(id)
        | Hit::Handle(id, _)
        | Hit::Minimize(id)
        | Hit::Maximize(id)
        | Hit::Close(id) => Some(id),
        _ => None,
    }
}

fn button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

struct HostGuard;

impl HostGuard {
    fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let guard = Self;
        crate::platform::enable_virtual_terminal_input();
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Hide,
        )?;
        Ok(guard)
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show,
        );
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn spawn_stdin_reader(events: Sender<DeskEvent>) {
    std::thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 8192];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if events.send(DeskEvent::Input(buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

pub(super) fn run() -> io::Result<i32> {
    let snap = match server::snapshot() {
        Ok(found) => found,
        Err(err) => {
            eprintln!("herdr desk: {err}");
            eprintln!("herdr desk needs a running herdr server; start one with `herdr server`.");
            return Ok(1);
        }
    };
    let (tx, rx) = mpsc::channel();
    let guard = HostGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let size = terminal.size()?;
    let mut app = App {
        tasks: Vec::new(),
        events: tx.clone(),
        desk: Desk::default(),
        data: HashMap::new(),
        screen: Rect::new(0, 0, size.width, size.height),
        hits: HitMap::default(),
        drag: None,
        content_press: None,
        last_title_click: None,
        prefix_armed: false,
        menu: None,
        dialog: None,
        quit: false,
        notice: None,
        stats: Stats::default(),
    };
    let reattached = snap.windows.len();
    app.apply(snap);
    if reattached > 1 {
        app.desk.tile_all(app.desk_rect());
        app.sync_sizes();
    }
    if app.desk.windows.is_empty() {
        app.new_window();
    }
    server::spawn_poller(tx.clone());
    spawn_stdin_reader(tx);
    let result = event_loop(&mut app, &mut terminal, &rx);
    for (_, data) in app.data.drain() {
        data.session.release();
    }
    drop(terminal);
    drop(guard);
    write_stats(&app.stats);
    result.map(|()| 0)
}

fn event_loop(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    rx: &Receiver<DeskEvent>,
) -> io::Result<()> {
    let mut framer = RawInputByteFramer::for_host_input();
    let mut dirty = true;
    let mut last_draw = Instant::now() - MIN_DRAW_INTERVAL;
    let mut last_input = Instant::now();
    while !app.quit {
        if dirty && last_draw.elapsed() >= MIN_DRAW_INTERVAL {
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            if screen != app.screen {
                app.screen = screen;
                app.desk.refit(app.desk_rect());
                app.sync_sizes();
            }
            let started = Instant::now();
            terminal.draw(|frame| app.draw(frame))?;
            let took = started.elapsed();
            app.stats.draws += 1;
            app.stats.draw_total += took;
            app.stats.draw_max = app.stats.draw_max.max(took);
            last_draw = Instant::now();
            dirty = false;
        }
        let timeout = if dirty {
            MIN_DRAW_INTERVAL.saturating_sub(last_draw.elapsed())
        } else if framer.has_pending_input() {
            INPUT_FLUSH
        } else {
            IDLE_TICK
        };
        match rx.recv_timeout(timeout) {
            Ok(event) => {
                if matches!(event, DeskEvent::Input(_)) {
                    last_input = Instant::now();
                }
                app.handle_event(event, &mut framer);
                while let Ok(event) = rx.try_recv() {
                    app.handle_event(event, &mut framer);
                }
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {
                if framer.has_pending_input() && last_input.elapsed() >= INPUT_FLUSH {
                    for chunk in framer.flush_timeout() {
                        app.handle_chunk(&chunk);
                    }
                    dirty = true;
                }
                // Poll for host resizes; raw input carries no resize event.
                if let Ok(size) = terminal.size() {
                    if Rect::new(0, 0, size.width, size.height) != app.screen {
                        dirty = true;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if app.desk.windows.is_empty() && app.data.is_empty() && !app.quit {
            dirty = true;
        }
    }
    Ok(())
}

/// `HERDR_DESK_STATS=<path>` writes frame and draw counters on exit (spike measurements).
fn write_stats(stats: &Stats) {
    let Some(path) = std::env::var_os("HERDR_DESK_STATS") else {
        return;
    };
    let avg_us = if stats.draws == 0 {
        0
    } else {
        stats.draw_total.as_micros() / u128::from(stats.draws)
    };
    let line = serde_json::json!({
        "frames": stats.frames,
        "frame_bytes": stats.frame_bytes,
        "draws": stats.draws,
        "draw_avg_us": avg_us,
        "draw_max_us": stats.draw_max.as_micros(),
    });
    if let Ok(mut file) = std::fs::File::create(path) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_menu_opens_above_the_taskbar_with_shell_actions() {
        let menu = start_menu((0, 35));
        let actions: Vec<Action> = menu.items.iter().map(|i| i.action.clone()).collect();
        assert_eq!(
            actions,
            [
                Action::NewWindow,
                Action::NewTask,
                Action::TileAll,
                Action::Quit
            ]
        );
        let desk = desk_area(Rect::new(0, 0, 120, 36));
        let rect = menu.rect(desk).unwrap();
        assert_eq!((rect.x, rect.bottom()), (0, 35));
    }
}
