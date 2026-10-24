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

use super::session::{self, DeskEvent, DeskPane, Session};
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
    Maximize(WinId),
    Close(WinId),
    NewWindow,
    TileAll,
    Quit,
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
    pane: DeskPane,
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
    workspace_id: String,
    events: Sender<DeskEvent>,
    desk: Desk,
    data: HashMap<WinId, WinData>,
    screen: Rect,
    hits: HitMap<Hit>,
    drag: Option<Drag>,
    content_press: Option<WinId>,
    last_title_click: Option<(WinId, Instant)>,
    prefix_armed: bool,
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

    fn attach(&mut self, pane: DeskPane, rect: Option<Rect>) -> io::Result<WinId> {
        let desk = self.desk_rect();
        let id = self.desk.open(pane.title.clone(), NEW_WINDOW_SIZE, desk);
        if let (Some(rect), Some(w)) = (rect, self.desk.get_mut(id)) {
            w.rect = wm::fit(rect, desk);
        }
        let inner = self.desk.get(id).map(|w| w.inner()).unwrap_or_default();
        let size = (inner.width.max(1), inner.height.max(1));
        let started = Mirror::new(size.0, size.1).and_then(|mirror| {
            Session::spawn(id, &pane.terminal_id, size.0, size.1, self.events.clone())
                .map(|session| (mirror, session))
        });
        match started {
            Ok((mirror, session)) => {
                self.data.insert(
                    id,
                    WinData {
                        pane,
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

    fn new_window(&mut self) {
        let created =
            session::create_pane(&self.workspace_id).and_then(|pane| self.attach(pane, None));
        if let Err(err) = created {
            self.notice = Some(format!("new window failed: {err}"));
        }
    }

    fn close_window(&mut self, id: WinId) {
        self.desk.close(id);
        if let Some(data) = self.data.remove(&id) {
            data.session.release();
            session::close_pane(&data.pane.pane_id);
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
            DeskEvent::Closed(win) => {
                // The pane exited (or the stream ended): drop the window, keep the pane alone.
                self.desk.close(win);
                if let Some(data) = self.data.remove(&win) {
                    data.session.release();
                }
            }
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
                    (Some(Hit::Maximize(id)), MouseButton::Left) => {
                        self.desk.toggle_maximize(id, desk);
                        self.sync_sizes();
                    }
                    (Some(Hit::Close(id)), MouseButton::Left) => self.close_window(id),
                    (Some(Hit::NewWindow), MouseButton::Left) => self.new_window(),
                    (Some(Hit::TileAll), MouseButton::Left) => {
                        self.desk.tile_all(desk);
                        self.sync_sizes();
                    }
                    (Some(Hit::Quit), MouseButton::Left) => self.quit = true,
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

        for w in &self.desk.windows {
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
            // Title bar: title on the left, [□][×] on the right.
            let buttons_w = 6u16;
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
            for (label, hit) in [
                (" ＋ New ", Hit::NewWindow),
                (" ▦ Tile ", Hit::TileAll),
                (" ⏻ Quit ", Hit::Quit),
            ] {
                let width = super::text_width(label);
                buf.set_string(
                    x,
                    bar.y,
                    label,
                    Style::default().bg(BAR_BG).fg(Color::White),
                );
                hits.push(Rect::new(x, bar.y, width, 1), hit);
                x += width + 1;
            }
            let hint = match (&self.notice, self.prefix_armed) {
                (_, true) => {
                    "desk: n new · w close · m max · t tile · Tab next · q quit".to_string()
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
        self.hits = hits;
    }
}

fn hit_window(hit: Option<Hit>) -> Option<WinId> {
    match hit? {
        Hit::Content(id)
        | Hit::Title(id)
        | Hit::Handle(id, _)
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
        super::platform_input::enable_vt_input();
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
    let (workspace_id, panes) = match session::ensure_workspace() {
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
        workspace_id,
        events: tx.clone(),
        desk: Desk::default(),
        data: HashMap::new(),
        screen: Rect::new(0, 0, size.width, size.height),
        hits: HitMap::default(),
        drag: None,
        content_press: None,
        last_title_click: None,
        prefix_armed: false,
        quit: false,
        notice: None,
        stats: Stats::default(),
    };
    let reattached = panes.len();
    for pane in panes {
        if let Err(err) = app.attach(pane, None) {
            app.notice = Some(format!("reattach failed: {err}"));
        }
    }
    if reattached > 1 {
        app.desk.tile_all(app.desk_rect());
        app.sync_sizes();
    }
    if app.desk.windows.is_empty() {
        app.new_window();
    }
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
