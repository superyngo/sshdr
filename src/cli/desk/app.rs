//! Desk event loop: raw host input, window drag sessions, per-window session streams, and
//! composition of the mirrored terminals into one frame.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
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
use super::folder::{self, Folder};
use super::menu::{Menu, MenuHit, MenuItem};
use super::server::{self, Snapshot, TaskInfo, Wake, WindowInfo};
use super::state;
use super::stream::{DeskEvent, Session};
use super::taskbar;
use super::transport::{self, Link, Target, LOCAL_KEY};
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
    /// The **Machines** desktop icon.
    Icon,
    /// A row of the **Machines Folder**.
    FolderRow(usize),
}

/// What a menu row does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Raise(WinId),
    Minimize(WinId),
    ToggleMaximize(WinId),
    Close(WinId),
    RenameWindow(WinId),
    NewWindowIn(TaskRef),
    NewWindow,
    NewTask,
    TileAll,
    Quit,
    RenameTask(TaskRef),
    MinimizeTask(TaskRef),
    RestoreTask(TaskRef),
    CloseTask(TaskRef),
    OpenFolder,
    /// Double-click in the **Machines Folder**: connect, or show what is already open.
    OpenMachine(String),
    Connect(String),
    Disconnect(String),
    /// **SSH Tier**: a local window running `ssh`.
    OpenSsh(String),
}

/// A **Task** on a **Target**: (target key, workspace id).
type TaskRef = (String, String);

/// What a dialog's primary button does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DialogTarget {
    RenameWindow(WinId),
    RenameTask(TaskRef),
    CloseTask(TaskRef),
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
    /// Key of the window's **Target**.
    target: String,
    info: WindowInfo,
    attached_at: Instant,
    /// `None` while the stream is down; the window keeps its place until the tab is gone.
    session: Option<Session>,
    /// ControlMaster index of the stream (`transport::STREAMS_PER_MASTER`).
    master: usize,
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

/// A **Task** with the key of its **Target**.
struct Task {
    target: String,
    info: TaskInfo,
}

/// Local, or a saved **Machine**, with its connection state.
struct TargetState {
    target: Target,
    link: Link,
    /// Set while connecting or connected; stops the watcher.
    watcher: Option<Watcher>,
    /// The first snapshot arrived (**Server Tier**).
    connected: bool,
}

/// Handle of a `server::spawn_watcher` thread.
struct Watcher {
    stop: Arc<AtomicBool>,
    wake: Sender<Wake>,
}

impl Watcher {
    fn spawn(target: Target, report_connect: bool, events: Sender<DeskEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, rx) = mpsc::channel();
        server::spawn_watcher(
            target,
            report_connect,
            stop.clone(),
            (wake.clone(), rx),
            events,
        );
        Self { stop, wake }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct App {
    tasks: Vec<Task>,
    targets: Vec<TargetState>,
    /// `desk.json` as loaded; geometry for windows that open after startup.
    saved: state::DeskState,
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
    folder: Option<Folder>,
    last_icon_click: Option<Instant>,
    last_row_click: Option<(usize, Instant)>,
    /// **Machines** connected by hand: an **SSH Tier** result opens an `ssh` window.
    ssh_on_tier: Vec<String>,
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

    fn target(&self, key: &str) -> Option<&Target> {
        self.targets
            .iter()
            .find(|t| t.target.key() == key)
            .map(|t| &t.target)
    }

    fn target_state(&mut self, key: &str) -> Option<&mut TargetState> {
        self.targets.iter_mut().find(|t| t.target.key() == key)
    }

    /// The ControlMaster for a new stream to `key`: the first with room (B22 results §4.4).
    fn pick_master(&self, key: &str) -> usize {
        let mut used: HashMap<usize, usize> = HashMap::new();
        for data in self.data.values() {
            if data.target == key && data.session.is_some() {
                *used.entry(data.master).or_default() += 1;
            }
        }
        (0..)
            .find(|m| used.get(m).copied().unwrap_or(0) < transport::STREAMS_PER_MASTER)
            .unwrap_or(0)
    }

    fn spawn_session(
        &self,
        id: WinId,
        key: &str,
        terminal_id: &str,
    ) -> io::Result<(Session, usize)> {
        let target = self
            .target(key)
            .ok_or_else(|| io::Error::other(format!("unknown machine {key}")))?;
        let master = match target {
            Target::Local => 0,
            Target::Machine(_) => self.pick_master(key),
        };
        transport::ensure_master(target, master)?;
        let size = self
            .desk
            .get(id)
            .map(|w| w.inner())
            .map(|r| (r.width.max(1), r.height.max(1)))
            .unwrap_or((1, 1));
        Session::spawn(
            id,
            target,
            master,
            terminal_id,
            size.0,
            size.1,
            self.events.clone(),
        )
        .map(|session| (session, master))
    }

    fn attach(&mut self, key: &str, info: WindowInfo, rect: Option<Rect>) -> io::Result<WinId> {
        let desk = self.desk_rect();
        let title = self.window_title(key, &info, false);
        let id = self.desk.open(title, NEW_WINDOW_SIZE, desk);
        let saved = self
            .saved
            .windows
            .iter()
            .find(|s| s.terminal_id == state::key(key, &info.terminal_id))
            .cloned();
        if let Some(w) = self.desk.get_mut(id) {
            match (rect, saved) {
                (Some(rect), _) => w.rect = wm::fit(rect, desk),
                (None, Some(saved)) => state::apply_saved(w, &saved, desk),
                (None, None) => {}
            }
        }
        let inner = self.desk.get(id).map(|w| w.inner()).unwrap_or_default();
        let size = (inner.width.max(1), inner.height.max(1));
        let started = Mirror::new(size.0, size.1).and_then(|mirror| {
            self.spawn_session(id, key, &info.terminal_id)
                .map(|session| (mirror, session))
        });
        match started {
            Ok((mirror, (session, master))) => {
                self.data.insert(
                    id,
                    WinData {
                        target: key.to_string(),
                        info,
                        attached_at: Instant::now(),
                        session: Some(session),
                        master,
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

    /// Restart the stream of a window whose stream ended, in place.
    fn reattach(&mut self, id: WinId) {
        let Some((key, terminal_id)) = self
            .data
            .get(&id)
            .map(|d| (d.target.clone(), d.info.terminal_id.clone()))
        else {
            return;
        };
        match self.spawn_session(id, &key, &terminal_id) {
            Ok((session, master)) => {
                let inner = self.desk.get(id).map(|w| w.inner()).unwrap_or_default();
                if let Some(data) = self.data.get_mut(&id) {
                    data.session = Some(session);
                    data.master = master;
                    data.sent_size = (inner.width.max(1), inner.height.max(1));
                }
                self.refresh_title(id);
            }
            Err(err) => tracing::warn!(%err, "desk: reattach failed"),
        }
    }

    /// `desk.json` keys of open windows.
    fn state_keys(&self) -> HashMap<WinId, String> {
        self.data
            .iter()
            .map(|(id, data)| (*id, state::key(&data.target, &data.info.terminal_id)))
            .collect()
    }

    fn win_id(&self, key: &str, tab_id: &str) -> Option<WinId> {
        self.data
            .iter()
            .find(|(_, data)| data.target == key && data.info.tab_id == tab_id)
            .map(|(id, _)| *id)
    }

    /// The focused window's **Task**, else the first local one.
    fn current_task(&self) -> Option<TaskRef> {
        self.desk
            .focus
            .and_then(|id| self.data.get(&id))
            .map(|data| (data.target.clone(), data.info.workspace_id.clone()))
            .or_else(|| {
                self.tasks
                    .iter()
                    .find(|t| t.target == LOCAL_KEY)
                    .map(|t| (t.target.clone(), t.info.workspace_id.clone()))
            })
    }

    /// New window in the current **Task**; creates a local **Task** when there is none.
    fn new_window(&mut self) {
        let created = match self.current_task() {
            Some((key, ws)) => self.new_window_in(&key, &ws),
            None => self.new_task(LOCAL_KEY, None),
        };
        if let Err(err) = created {
            self.notice = Some(format!("new window failed: {err}"));
        }
    }

    fn new_window_in(&mut self, key: &str, ws: &str) -> io::Result<WinId> {
        let target = self
            .target(key)
            .cloned()
            .ok_or_else(|| io::Error::other("machine not connected"))?;
        let info = server::create_window(&target, ws)?;
        self.attach(key, info, None)
    }

    fn new_task(&mut self, key: &str, label: Option<&str>) -> io::Result<WinId> {
        let target = self
            .target(key)
            .cloned()
            .ok_or_else(|| io::Error::other("machine not connected"))?;
        let (task, info) = server::create_task(&target, label)?;
        self.tasks.push(Task {
            target: key.to_string(),
            info: task,
        });
        self.sort_tasks();
        self.attach(key, info, None)
    }

    /// Local **Tasks** first, then each **Machine**'s in Machines Folder order.
    fn sort_tasks(&mut self) {
        let order: Vec<String> = self
            .targets
            .iter()
            .map(|t| t.target.key().to_string())
            .collect();
        self.tasks.sort_by_key(|t| {
            order
                .iter()
                .position(|k| *k == t.target)
                .unwrap_or(usize::MAX)
        });
    }

    /// Reconcile one **Target**'s windows with its server state: new tabs open, closed tabs go,
    /// labels follow, windows whose stream ended reattach in place.
    fn apply(&mut self, key: &str, snap: Snapshot) {
        match self.target_state(key) {
            // A late snapshot from a stopped watcher.
            Some(state) if state.watcher.is_none() && key != LOCAL_KEY => return,
            Some(state) => {
                state.link = Link::Server;
                state.connected = true;
            }
            None => return,
        }
        self.tasks.retain(|t| t.target != key);
        self.tasks.extend(snap.tasks.into_iter().map(|info| Task {
            target: key.to_string(),
            info,
        }));
        self.sort_tasks();
        for info in snap.windows.iter().cloned() {
            match self.win_id(key, &info.tab_id) {
                Some(id) if self.data[&id].info.terminal_id == info.terminal_id => {
                    if let Some(data) = self.data.get_mut(&id) {
                        data.info = info;
                    }
                    if self.data[&id].session.is_none() {
                        self.reattach(id);
                    }
                    self.refresh_title(id);
                }
                found => {
                    let rect = found.and_then(|id| self.desk.get(id).map(|w| w.rect));
                    if let Some(id) = found {
                        self.drop_window(id);
                    }
                    if let Err(err) = self.attach(key, info, rect) {
                        self.notice = Some(format!("open window failed: {err}"));
                    }
                }
            }
        }
        // A snapshot taken before a window was created must not drop it.
        let grace = server::CREATE_GRACE;
        let gone: Vec<WinId> = self
            .data
            .iter()
            .filter(|(_, data)| {
                data.target == key
                    && data.attached_at.elapsed() > grace
                    && !snap.windows.iter().any(|w| w.tab_id == data.info.tab_id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.drop_window(id);
        }
    }

    /// A poll failed: keep the windows, show the link state.
    fn set_link(&mut self, key: &str, link: Link) {
        if let Some(state) = self.target_state(key) {
            state.link = link;
        }
        let ids: Vec<WinId> = self
            .data
            .iter()
            .filter(|(_, d)| d.target == key)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.refresh_title(id);
        }
    }

    fn link(&self, key: &str) -> Link {
        self.targets
            .iter()
            .find(|t| t.target.key() == key)
            .map_or(Link::NotConnected, |t| t.link)
    }

    /// Follow a **Target**: the local **Server** from start, a **Machine** once connected.
    /// For a **Machine** the first result arrives as [`DeskEvent::Connected`] (**Tier**
    /// detection, ADR 0004).
    fn connect(&mut self, key: &str) {
        let events = self.events.clone();
        let Some(state) = self.target_state(key) else {
            return;
        };
        if state.watcher.is_some() {
            return;
        }
        let report_connect = key != LOCAL_KEY;
        if report_connect {
            state.link = Link::Connecting;
        }
        state.watcher = Some(Watcher::spawn(state.target.clone(), report_connect, events));
    }

    /// Ask a **Target**'s watcher for a fresh snapshot (a window stream ended).
    fn refresh(&mut self, key: &str) {
        if let Some(watcher) = self.target_state(key).and_then(|s| s.watcher.as_ref()) {
            let _ = watcher.wake.send(Wake::Refresh);
        }
    }

    fn connected(&mut self, key: &str, result: Result<Snapshot, Link>) {
        match result {
            Ok(snap) => {
                self.apply(key, snap);
            }
            Err(link) => {
                if let Some(state) = self.target_state(key) {
                    state.watcher = None;
                }
                self.set_link(key, link);
                let by_hand = self.ssh_on_tier.iter().any(|k| k == key);
                self.ssh_on_tier.retain(|k| k != key);
                if link == Link::Ssh && by_hand {
                    self.open_ssh(key);
                }
            }
        }
    }

    /// **SSH Tier**: a new window in the local **Task** named after the **Machine**, running
    /// `ssh <target>` (plan decision 3A).
    fn open_ssh(&mut self, key: &str) {
        let Some(Target::Machine(machine)) = self.target(key).cloned() else {
            return;
        };
        let task = self
            .tasks
            .iter()
            .find(|t| t.target == LOCAL_KEY && t.info.label == machine.label)
            .map(|t| t.info.workspace_id.clone());
        let opened = match task {
            Some(ws) => self.new_window_in(LOCAL_KEY, &ws),
            None => self.new_task(LOCAL_KEY, Some(&machine.label)),
        };
        match opened {
            Ok(id) => {
                if let Some(pane) = self.data.get(&id).map(|d| d.info.pane_id.clone()) {
                    let command = format!("ssh {}", transport::quote(&machine.target));
                    server::run_in_pane(&Target::Local, &pane, &command);
                }
            }
            Err(err) => self.notice = Some(format!("ssh window failed: {err}")),
        }
    }

    /// Rows of the **Machines Folder**, in `targets` order.
    fn folder_rows(&self) -> Vec<folder::Row> {
        self.targets
            .iter()
            .map(|t| {
                let windows = self
                    .data
                    .values()
                    .filter(|d| d.target == t.target.key())
                    .count();
                let (target, status) = match &t.target {
                    Target::Local => ("this computer".to_string(), "Local".to_string()),
                    Target::Machine(m) => (m.target.clone(), t.link.text().to_string()),
                };
                let status = if t.connected && windows > 0 {
                    format!("{status} · {windows}")
                } else {
                    status
                };
                folder::Row {
                    name: t.target.label().to_string(),
                    target,
                    status,
                    alert: matches!(t.link, Link::Auth | Link::Offline),
                }
            })
            .collect()
    }

    fn open_folder(&mut self) {
        if let Some(f) = self.folder {
            if self.desk.get(f.win).is_some() {
                self.desk.raise(f.win);
                return;
            }
        }
        let size = folder::size(&self.folder_rows());
        let win = self
            .desk
            .open(folder::TITLE.to_string(), size, self.desk_rect());
        self.folder = Some(Folder { win, selected: 0 });
    }

    /// Double-click or Enter on a folder row.
    fn open_machine(&mut self, key: &str) {
        let Some(state) = self.targets.iter().find(|t| t.target.key() == key) else {
            return;
        };
        match (state.link, state.connected) {
            (Link::Ssh, false) => self.open_ssh(key),
            (Link::Connecting, _) => {}
            (_, true) => {
                // Connected (or Local): bring its windows forward.
                let ids: Vec<WinId> = self
                    .desk
                    .windows
                    .iter()
                    .map(|w| w.id)
                    .filter(|id| self.data.get(id).is_some_and(|d| d.target == key))
                    .collect();
                for id in ids {
                    self.desk.raise(id);
                }
            }
            (_, false) => {
                self.ssh_on_tier.push(key.to_string());
                self.connect(key);
            }
        }
    }

    fn folder_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let state = self.targets.get(index)?;
        let key = state.target.key().to_string();
        let mut items = Vec::new();
        match (&state.target, state.connected) {
            (Target::Local, _) => {
                items.push(MenuItem::new("Show windows", "", Action::OpenMachine(key)))
            }
            (Target::Machine(_), true) => {
                items.push(MenuItem::new(
                    "Show windows",
                    "",
                    Action::OpenMachine(key.clone()),
                ));
                items.push(MenuItem::new("Disconnect", "", Action::Disconnect(key)));
            }
            (Target::Machine(_), false) => {
                items.push(MenuItem::new("Connect", "", Action::Connect(key.clone())));
                items.push(MenuItem::new("Open ssh window", "", Action::OpenSsh(key)));
            }
        }
        Menu::open(state.target.label().to_string(), at, items)
    }

    /// Keys while the **Machines Folder** has focus.
    fn folder_key(&mut self, chunk: &[u8]) {
        let Some(f) = self.folder.as_mut() else {
            return;
        };
        let last = self.targets.len().saturating_sub(1);
        match chunk {
            b"\x1b[A" | b"\x1bOA" => f.selected = f.selected.saturating_sub(1),
            b"\x1b[B" | b"\x1bOB" => f.selected = (f.selected + 1).min(last),
            b"\r" | b"\n" => {
                let key = self
                    .targets
                    .get(f.selected)
                    .map(|t| t.target.key().to_string());
                if let Some(key) = key {
                    self.open_machine(&key);
                }
            }
            _ => {}
        }
    }

    /// Stop polling a **Machine** and close its windows; its tabs stay on its **Server**.
    fn disconnect(&mut self, key: &str) {
        let Some(state) = self.target_state(key) else {
            return;
        };
        if let Some(watcher) = state.watcher.take() {
            watcher.stop();
        }
        state.connected = false;
        state.link = Link::NotConnected;
        let ids: Vec<WinId> = self
            .data
            .iter()
            .filter(|(_, d)| d.target == key)
            .map(|(id, _)| *id)
            .collect();
        // Remember their geometry for the next connection.
        let keys = self.state_keys();
        let captured = state::capture(&self.desk, |id| {
            ids.contains(&id).then(|| keys.get(&id).cloned()).flatten()
        });
        self.saved.windows.retain(|s| {
            !captured
                .windows
                .iter()
                .any(|c| c.terminal_id == s.terminal_id)
        });
        self.saved.windows.extend(captured.windows);
        for id in ids {
            self.drop_window(id);
        }
        self.tasks.retain(|t| t.target != key);
    }

    /// `desk.json` contents: open windows, plus saved windows of **Machines** not connected now.
    fn capture(&self) -> state::DeskState {
        let keys = self.state_keys();
        let mut out = state::capture(&self.desk, |id| keys.get(&id).cloned());
        let connected: Vec<&str> = self
            .targets
            .iter()
            .filter(|t| t.connected)
            .map(|t| t.target.key())
            .collect();
        let kept: Vec<state::SavedWindow> = self
            .saved
            .windows
            .iter()
            .filter(|s| {
                state::key_machine(&s.terminal_id).is_some_and(|m| !connected.contains(&m))
                    && !out.windows.iter().any(|o| o.terminal_id == s.terminal_id)
            })
            .cloned()
            .collect();
        out.windows.splice(0..0, kept);
        out.machines = connected
            .into_iter()
            .filter(|key| *key != LOCAL_KEY)
            .map(str::to_string)
            .collect();
        out
    }

    fn window_title(&self, key: &str, info: &WindowInfo, offline: bool) -> String {
        let mut title = window_title(info);
        if key != LOCAL_KEY {
            let host = self.target(key).map_or(key, Target::label);
            title = format!("{host}: {title}");
        }
        if offline {
            title = format!("⚠ {title}");
        }
        title
    }

    fn refresh_title(&mut self, id: WinId) {
        let Some(data) = self.data.get(&id) else {
            return;
        };
        let offline =
            data.session.is_none() || matches!(self.link(&data.target), Link::Offline | Link::Auth);
        let title = self.window_title(&data.target, &data.info, offline);
        if let Some(w) = self.desk.get_mut(id) {
            w.title = title;
        }
    }

    /// Remove a window from the desk; its tab stays on the server.
    fn drop_window(&mut self, id: WinId) {
        self.desk.close(id);
        if let Some(session) = self.data.remove(&id).and_then(|data| data.session) {
            session.release();
        }
    }

    fn close_window(&mut self, id: WinId) {
        self.desk.close(id);
        if self.folder.is_some_and(|f| f.win == id) {
            self.folder = None;
        }
        if let Some(data) = self.data.remove(&id) {
            if let Some(session) = data.session {
                session.release();
            }
            if let Some(target) = self.target(&data.target) {
                server::close_window(target, &data.info.tab_id);
            }
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
                if let (true, Some(session)) = (data.sent_size != size, data.session.as_mut()) {
                    data.sent_size = size;
                    session.resize(size.0, size.1);
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
            // The stream ended: keep the window until the next snapshot says whether its tab
            // is gone (drop) or still there (reattach in place).
            DeskEvent::Closed(win, serial) => {
                let ended = self
                    .data
                    .get_mut(&win)
                    .filter(|d| d.session.as_ref().is_some_and(|s| s.serial == serial))
                    .and_then(|d| d.session.take());
                if let Some(session) = ended {
                    session.release();
                    self.refresh_title(win);
                    if let Some(key) = self.data.get(&win).map(|d| d.target.clone()) {
                        self.refresh(&key);
                    }
                }
            }
            DeskEvent::Snapshot(key, snap) => self.apply(&key, snap),
            DeskEvent::Link(key, link) => self.set_link(&key, link),
            DeskEvent::Connected(key, result) => self.connected(&key, result),
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
        if self.desk.focus.is_some() && self.desk.focus == self.folder.map(|f| f.win) {
            self.folder_key(chunk);
            return;
        }
        self.forward_input(chunk);
    }

    fn forward_input(&mut self, bytes: &[u8]) {
        if let Some(session) = self
            .desk
            .focus
            .and_then(|id| self.data.get_mut(&id))
            .and_then(|data| data.session.as_mut())
        {
            session.input(bytes);
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
        if let Some(session) = self.data.get_mut(&id).and_then(|d| d.session.as_mut()) {
            session.mouse(action, button, x, y, 0);
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
                    (Some(Hit::Icon), MouseButton::Left) => {
                        let now = Instant::now();
                        let double = self
                            .last_icon_click
                            .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
                        self.last_icon_click = Some(now);
                        if double {
                            self.open_folder();
                        }
                    }
                    (Some(Hit::FolderRow(index)), button) => {
                        if let Some(f) = self.folder.as_mut() {
                            self.desk.raise(f.win);
                            f.selected = index;
                        }
                        let now = Instant::now();
                        let double = matches!(self.last_row_click,
                            Some((last, at)) if last == index && now.duration_since(at) < DOUBLE_CLICK);
                        self.last_row_click = Some((index, now));
                        let key = self.targets.get(index).map(|t| t.target.key().to_string());
                        match (button, key) {
                            (MouseButton::Right, _) => {
                                self.menu = self.folder_menu(index, (column, row));
                            }
                            (MouseButton::Left, Some(key)) if double => self.open_machine(&key),
                            _ => {}
                        }
                    }
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
                        if let Some(session) =
                            self.data.get_mut(&id).and_then(|d| d.session.as_mut())
                        {
                            session.scroll(up, x, y);
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
        hits.push(folder::render_icon(desk, frame.buffer_mut()), Hit::Icon);
        let folder_rows = self.folder.map(|_| self.folder_rows());

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
            let folder_rows = match (self.folder, &folder_rows) {
                (Some(f), Some(rows)) if f.win == w.id => Some((f.selected, rows)),
                _ => None,
            };
            let mut row_hits = Vec::new();
            if let Some((selected, rows)) = folder_rows {
                folder::render(rows, selected, inner, frame.buffer_mut(), |rect, i| {
                    row_hits.push((rect, i));
                });
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
            for (rect, i) in row_hits {
                hits.push(rect, Hit::FolderRow(i));
            }
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
                    .filter(|d| {
                        d.target == task.target && d.info.workspace_id == task.info.workspace_id
                    })
                    .count();
                taskbar::task_text(&self.task_title(task), count)
            });
            let buttons = taskbar::layout(x + 1, bar.right(), texts.collect::<Vec<_>>());
            for (index, button) in buttons.iter().enumerate() {
                let task = &self.tasks[index];
                let active = current
                    .as_ref()
                    .is_some_and(|(key, ws)| *key == task.target && *ws == task.info.workspace_id);
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
                (None, false) => format!(
                    "{}{} windows · Ctrl+\\ desk keys",
                    self.link_summary(),
                    self.desk.windows.len()
                ),
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
                self.data.get(&w.id).is_some_and(|d| {
                    d.target == task.target && d.info.workspace_id == task.info.workspace_id
                })
            })
            .map(|w| {
                let mark = if w.minimized { "▁ " } else { "  " };
                MenuItem::new(format!("{mark}{}", w.title), "", Action::Raise(w.id))
            })
            .collect();
        Menu::open(self.task_title(task), at, items).map(Menu::above)
    }

    fn task_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let task = self.tasks.get(index)?;
        let ws: TaskRef = (task.target.clone(), task.info.workspace_id.clone());
        Menu::open(
            self.task_title(task),
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

    fn task_windows(&self, (key, ws): &TaskRef) -> Vec<WinId> {
        self.desk
            .windows
            .iter()
            .map(|w| w.id)
            .filter(|id| {
                self.data
                    .get(id)
                    .is_some_and(|d| d.target == *key && d.info.workspace_id == *ws)
            })
            .collect()
    }

    fn find_task(&self, (key, ws): &TaskRef) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| t.target == *key && t.info.workspace_id == *ws)
    }

    fn task_label(&self, task: &TaskRef) -> String {
        self.find_task(task)
            .map(|t| t.info.label.clone())
            .unwrap_or_default()
    }

    /// Taskbar text of a **Task**: remote ones carry the **Machine** name.
    fn task_title(&self, task: &Task) -> String {
        match self.target(&task.target) {
            Some(target @ Target::Machine(_)) => format!("{}: {}", target.label(), task.info.label),
            _ => task.info.label.clone(),
        }
    }

    /// `omarchy ! offline · ` for connected **Machines** that are not reachable.
    fn link_summary(&self) -> String {
        self.targets
            .iter()
            .filter(|t| t.connected && matches!(t.link, Link::Offline | Link::Auth))
            .map(|t| format!("{} {} · ", t.target.label(), t.link.text()))
            .collect()
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
                        DialogTarget::RenameWindow(id),
                    ));
                }
            }
            Action::NewWindow => self.new_window(),
            Action::NewTask => {
                if let Err(err) = self.new_task(LOCAL_KEY, None) {
                    self.notice = Some(format!("new task failed: {err}"));
                }
            }
            Action::TileAll => {
                self.desk.tile_all(desk);
                self.sync_sizes();
            }
            Action::Quit => self.quit = true,
            Action::OpenFolder => self.open_folder(),
            Action::OpenMachine(key) => self.open_machine(&key),
            Action::Connect(key) => {
                self.ssh_on_tier.push(key.clone());
                self.connect(&key);
            }
            Action::Disconnect(key) => self.disconnect(&key),
            Action::OpenSsh(key) => self.open_ssh(&key),
            Action::NewWindowIn((key, ws)) => {
                if let Err(err) = self.new_window_in(&key, &ws) {
                    self.notice = Some(format!("new window failed: {err}"));
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
            DialogTarget::RenameWindow(id) if !text.is_empty() => {
                let Some(data) = self.data.get_mut(&id) else {
                    return;
                };
                data.info.label = text.to_string();
                let (key, tab_id) = (data.target.clone(), data.info.tab_id.clone());
                if let Some(target) = self.target(&key) {
                    server::rename_window(target, &tab_id, text);
                }
                self.refresh_title(id);
            }
            DialogTarget::RenameTask(task) if !text.is_empty() => {
                if let Some(target) = self.target(&task.0) {
                    server::rename_task(target, &task.1, text);
                }
                if let Some(found) = self
                    .tasks
                    .iter_mut()
                    .find(|t| t.target == task.0 && t.info.workspace_id == task.1)
                {
                    found.info.label = text.to_string();
                }
            }
            DialogTarget::CloseTask(task) => {
                for id in self.task_windows(&task) {
                    self.drop_window(id);
                }
                self.tasks
                    .retain(|t| !(t.target == task.0 && t.info.workspace_id == task.1));
                if let Some(target) = self.target(&task.0) {
                    server::close_task(target, &task.1);
                }
            }
            _ => {}
        }
    }
}

const START_TITLE: &str = "Start";

/// The **Start Menu**, opening upward from the Start button.
fn start_menu(at: (u16, u16)) -> Menu<Action> {
    let items = vec![
        MenuItem::new("Machines", "", Action::OpenFolder),
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
    let snap = match server::snapshot(&Target::Local, server::CALL_TIMEOUT) {
        Ok(found) => found,
        Err(err) => {
            eprintln!("herdr desk: {}", err.message);
            eprintln!("herdr desk needs a running herdr server; start one with `herdr server`.");
            return Ok(1);
        }
    };
    let (tx, rx) = mpsc::channel();
    let guard = HostGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let size = terminal.size()?;
    let mut targets = vec![TargetState {
        target: Target::Local,
        link: Link::Server,
        watcher: None,
        connected: true,
    }];
    targets.extend(transport::machines().into_iter().map(|m| TargetState {
        target: Target::Machine(m),
        link: Link::NotConnected,
        watcher: None,
        connected: false,
    }));
    let mut app = App {
        tasks: Vec::new(),
        targets,
        saved: state::DeskState::default(),
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
        folder: None,
        last_icon_click: None,
        last_row_click: None,
        ssh_on_tier: Vec::new(),
    };
    let reattached = snap.windows.len();
    app.apply(LOCAL_KEY, snap);
    let state_path = state::path();
    let saved = state::load(&state_path);
    if !saved.windows.is_empty() {
        let area = app.desk_rect();
        let keys = app.state_keys();
        state::restore(&mut app.desk, &saved, |id| keys.get(&id).cloned(), area);
        app.sync_sizes();
    } else if reattached > 1 {
        app.desk.tile_all(app.desk_rect());
        app.sync_sizes();
    }
    if app.desk.windows.is_empty() {
        app.new_window();
    }
    app.connect(LOCAL_KEY);
    for machine in &saved.machines {
        app.connect(machine);
    }
    app.saved = saved;
    spawn_stdin_reader(tx);
    let result = event_loop(&mut app, &mut terminal, &rx);
    state::save(&state_path, &app.capture());
    for (_, data) in app.data.drain() {
        if let Some(session) = data.session {
            session.release();
        }
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
                Action::OpenFolder,
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
