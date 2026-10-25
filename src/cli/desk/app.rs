//! Desk event loop: raw host input, window drag sessions, per-window session streams, and
//! composition of the mirrored terminals into one frame.

use std::collections::{BTreeMap, HashMap};
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
use super::server::{self, Snapshot, TabInfo, Wake, WorkspaceInfo};
use super::state;
use super::stream::{DeskEvent, Session};
use super::taskbar::{self, Edge, Place};
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
    /// A pane (by index in the shown tab) of a window, or a **Native Window**'s content (0).
    Content(WinId, usize),
    Title(WinId),
    Handle(WinId, Edges),
    Minimize(WinId),
    Maximize(WinId),
    Close(WinId),
    /// A tab in a window's tab strip: click shows it, right click opens its menu.
    Tab(WinId, usize),
    /// The `×` of a tab.
    TabClose(WinId, usize),
    /// `+` after the tabs.
    NewTab(WinId),
    Start,
    Task(usize),
    Menu(MenuHit),
    Dialog(DialogButton),
    /// A row of the **Machines Folder**.
    FolderRow(usize),
    /// **Taskbar** space without a button: drag moves the bar, right click opens its menu.
    Bar,
    /// The desk line touching the **Taskbar**: drag resizes the bar.
    Grip,
}

/// What a menu row does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Raise(WinId),
    Minimize(WinId),
    ToggleMaximize(WinId),
    Close(WinId),
    RenameWindow(WinId),
    NewTab(WinId),
    RenameTab(WinId, String),
    CloseTab(WinId, String),
    /// New window (workspace) in the **Task** of this **Machine** key.
    NewWindowOn(String),
    NewWindow,
    TileAll,
    Quit,
    MinimizeTask(String),
    RestoreTask(String),
    TaskbarEdge(Edge),
    /// Grow (+1) or shrink (-1) the **Taskbar**.
    TaskbarSize(i8),
    OpenFolder,
    /// Double-click in the **Machines Folder**: connect, or show what is already open.
    OpenMachine(String),
    Connect(String),
    Disconnect(String),
    /// **SSH Tier**: a local window running `ssh`.
    OpenSsh(String),
}

/// What a dialog's primary button does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DialogTarget {
    RenameWindow(WinId),
    RenameTab(WinId, String),
    CloseWindow(WinId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move { grab: (u16, u16) },
    Resize { edges: Edges },
}

/// A **Taskbar** drag: moving it (with the edge it would land on) or resizing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarDrag {
    Move(Option<Edge>),
    Resize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: WinId,
    kind: DragKind,
    original: Rect,
    desk: Rect,
    zone: Option<Rect>,
}

/// A **Terminal Window**: one workspace (ADR 0008).
struct WinData {
    /// Key of the **Target** whose **Server** holds the workspace.
    target: String,
    /// Key of the **Task** (**Machine**) holding the window: `target`, or the **SSH Tier**
    /// **Machine** of a local `ssh` workspace.
    task: String,
    workspace: WorkspaceInfo,
    /// The workspace's tabs, in server order.
    tabs: Vec<TabInfo>,
    attached_at: Instant,
    /// Streams of the shown tab's panes.
    panes: Vec<PaneView>,
}

impl WinData {
    fn active_tab(&self) -> Option<&TabInfo> {
        self.tabs
            .iter()
            .find(|t| t.tab_id == self.workspace.active_tab_id)
            .or(self.tabs.first())
    }

    /// Index in `panes` of the shown tab's focused pane.
    fn focused_pane(&self) -> usize {
        self.active_tab()
            .and_then(|t| t.panes.iter().position(|p| p.pane_id == t.focused_pane_id))
            .unwrap_or(0)
    }

    fn active_index(&self) -> usize {
        self.tabs
            .iter()
            .position(|t| t.tab_id == self.workspace.active_tab_id)
            .unwrap_or(0)
    }

    fn pane_by_serial(&mut self, serial: u64) -> Option<&mut PaneView> {
        self.panes
            .iter_mut()
            .find(|p| p.session.as_ref().is_some_and(|s| s.serial == serial))
    }

    fn release(&mut self) {
        for pane in &mut self.panes {
            if let Some(session) = pane.session.take() {
                session.release();
            }
        }
    }
}

/// One pane of the shown tab, mirrored through its own stream.
struct PaneView {
    terminal_id: String,
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
    targets: Vec<TargetState>,
    /// `desk.json` as loaded; geometry for windows that open after startup.
    saved: state::DeskState,
    /// Local workspaces opened as `ssh` windows: workspace id → **Machine** key.
    ssh_workspaces: BTreeMap<String, String>,
    events: Sender<DeskEvent>,
    desk: Desk,
    data: HashMap<WinId, WinData>,
    screen: Rect,
    hits: HitMap<Hit>,
    drag: Option<Drag>,
    content_press: Option<(WinId, usize)>,
    last_title_click: Option<(WinId, Instant)>,
    prefix_armed: bool,
    menu: Option<Menu<Action>>,
    dialog: Option<Dialog<DialogTarget>>,
    quit: bool,
    notice: Option<String>,
    stats: Stats,
    folder: Option<Folder>,
    last_row_click: Option<(usize, Instant)>,
    /// **Machines** connected by hand: an **SSH Tier** result opens an `ssh` window.
    ssh_on_tier: Vec<String>,
    /// **Taskbar** edge and size (B21).
    bar: Place,
    bar_drag: Option<BarDrag>,
}

impl App {
    fn desk_rect(&self) -> Rect {
        self.bar.desk(self.screen)
    }

    /// Put the **Taskbar** at `place`; windows refit the new desk (maximized ones fill it).
    fn set_bar(&mut self, place: Place) {
        self.bar = place.sized(place.size, self.screen);
        self.desk.refit(self.desk_rect());
        self.sync_sizes();
    }

    fn bar_popup(&self, menu: Menu<Action>, at: (u16, u16)) -> Menu<Action> {
        bar_popup(self.bar, self.screen, menu, at)
    }

    fn taskbar_menu(&self, at: (u16, u16)) -> Option<Menu<Action>> {
        let mut items: Vec<MenuItem<Action>> = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right]
            .into_iter()
            .map(|edge| {
                let mark = if edge == self.bar.edge { "•" } else { "" };
                MenuItem::new(edge.label(), mark, Action::TaskbarEdge(edge))
            })
            .collect();
        items.push(MenuItem::new("Larger", "", Action::TaskbarSize(1)));
        items.push(MenuItem::new("Smaller", "", Action::TaskbarSize(-1)));
        Menu::open("Taskbar", at, items).map(|m| self.bar_popup(m, at))
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
        for data in self.data.values().filter(|d| d.target == key) {
            for pane in data.panes.iter().filter(|p| p.session.is_some()) {
                *used.entry(pane.master).or_default() += 1;
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
        size: (u16, u16),
    ) -> io::Result<(Session, usize)> {
        let target = self
            .target(key)
            .ok_or_else(|| io::Error::other(format!("unknown machine {key}")))?;
        let master = match target {
            Target::Local => 0,
            Target::Machine(_) => self.pick_master(key),
        };
        transport::ensure_master(target, master)?;
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

    /// Where a window shows its panes: the shown tab's layout, scaled into the content area
    /// below the tab strip.
    fn pane_rects(&self, id: WinId) -> Vec<Rect> {
        let area = self
            .desk
            .get(id)
            .map(|w| content_area(w.inner()))
            .unwrap_or_default();
        self.data
            .get(&id)
            .and_then(WinData::active_tab)
            .map(|tab| layout_rects(tab, area))
            .unwrap_or_default()
    }

    /// Open a window for a workspace and stream its shown tab.
    fn attach(
        &mut self,
        key: &str,
        workspace: WorkspaceInfo,
        tabs: Vec<TabInfo>,
        rect: Option<Rect>,
    ) -> io::Result<WinId> {
        let desk = self.desk_rect();
        let id = self.desk.open(String::new(), NEW_WINDOW_SIZE, desk);
        let saved = self
            .saved
            .windows
            .iter()
            .find(|s| s.workspace == state::key(key, &workspace.workspace_id))
            .cloned();
        if let Some(w) = self.desk.get_mut(id) {
            match (rect, saved) {
                (Some(rect), _) => w.rect = wm::fit(rect, desk),
                (None, Some(saved)) => state::apply_saved(w, &saved, desk),
                (None, None) => {}
            }
        }
        let task = self.task_of(key, &workspace.workspace_id);
        self.data.insert(
            id,
            WinData {
                target: key.to_string(),
                task,
                workspace,
                tabs,
                attached_at: Instant::now(),
                panes: Vec::new(),
            },
        );
        match self.show_panes(id) {
            Ok(()) => {
                self.refresh_title(id);
                Ok(id)
            }
            Err(err) => {
                self.drop_window(id);
                Err(err)
            }
        }
    }

    /// The **Task** of a workspace on `key`.
    fn task_of(&self, key: &str, workspace_id: &str) -> String {
        match self.ssh_workspaces.get(workspace_id) {
            Some(machine) if key == LOCAL_KEY => machine.clone(),
            _ => key.to_string(),
        }
    }

    /// Stream the panes of the window's shown tab: a new tab replaces the streams, a stream
    /// that ended is restarted in place.
    fn show_panes(&mut self, id: WinId) -> io::Result<()> {
        let Some(data) = self.data.get(&id) else {
            return Ok(());
        };
        let key = data.target.clone();
        let want: Vec<String> = data
            .active_tab()
            .map(|t| t.panes.iter().map(|p| p.terminal_id.clone()).collect())
            .unwrap_or_default();
        let rects = self.pane_rects(id);
        let size = |i: usize| {
            rects
                .get(i)
                .map_or((1, 1), |r| (r.width.max(1), r.height.max(1)))
        };
        let same = data.panes.iter().map(|p| &p.terminal_id).eq(want.iter());
        if !same {
            let mut panes = Vec::new();
            for (i, terminal_id) in want.iter().enumerate() {
                let (cols, rows) = size(i);
                panes.push(PaneView {
                    terminal_id: terminal_id.clone(),
                    session: None,
                    master: 0,
                    mirror: Mirror::new(cols, rows)?,
                    sent_size: (cols, rows),
                });
            }
            if let Some(data) = self.data.get_mut(&id) {
                data.release();
                data.panes = panes;
            }
        }
        let mut result = Ok(());
        for (i, terminal_id) in want.iter().enumerate() {
            let down = self
                .data
                .get(&id)
                .and_then(|d| d.panes.get(i))
                .is_some_and(|p| p.session.is_none());
            if !down {
                continue;
            }
            let started = self.spawn_session(id, &key, terminal_id, size(i));
            match started {
                Ok((session, master)) => {
                    if let Some(pane) = self.data.get_mut(&id).and_then(|d| d.panes.get_mut(i)) {
                        pane.session = Some(session);
                        pane.master = master;
                        pane.sent_size = size(i);
                    }
                }
                Err(err) => result = Err(err),
            }
        }
        result
    }

    /// `desk.json` keys of open windows.
    fn state_keys(&self) -> HashMap<WinId, String> {
        self.data
            .iter()
            .map(|(id, data)| (*id, state::key(&data.target, &data.workspace.workspace_id)))
            .collect()
    }

    fn win_id(&self, key: &str, workspace_id: &str) -> Option<WinId> {
        self.data
            .iter()
            .find(|(_, data)| data.target == key && data.workspace.workspace_id == workspace_id)
            .map(|(id, _)| *id)
    }

    /// The focused window's **Task**, else Local.
    fn current_task(&self) -> String {
        self.desk
            .focus
            .and_then(|id| self.data.get(&id))
            .map_or_else(|| LOCAL_KEY.to_string(), |data| data.task.clone())
    }

    /// New window in the current **Task**.
    fn new_window(&mut self) {
        let task = self.current_task();
        self.new_window_on(&task);
    }

    /// New window (workspace) in the **Task** of `key`; an **SSH Tier** **Machine** gets an
    /// `ssh` window.
    fn new_window_on(&mut self, key: &str) {
        let Some(state) = self.targets.iter().find(|t| t.target.key() == key) else {
            return;
        };
        if !state.connected {
            if matches!(state.target, Target::Machine(_)) {
                self.open_ssh(key);
            }
            return;
        }
        let target = state.target.clone();
        let created = server::create_workspace(&target, None)
            .and_then(|(workspace, tab)| self.attach(key, workspace, vec![tab], None));
        if let Err(err) = created {
            self.notice = Some(format!("new window failed: {err}"));
        }
    }

    /// New tab in a window, shown at once; an `ssh` window's tab runs `ssh` too.
    fn new_tab(&mut self, id: WinId) {
        let Some(data) = self.data.get(&id) else {
            return;
        };
        let (key, task, workspace_id) = (
            data.target.clone(),
            data.task.clone(),
            data.workspace.workspace_id.clone(),
        );
        let Some(target) = self.target(&key).cloned() else {
            return;
        };
        let tab = match server::create_tab(&target, &workspace_id) {
            Ok(tab) => tab,
            Err(err) => {
                self.notice = Some(format!("new tab failed: {err}"));
                return;
            }
        };
        if task != key {
            if let Some(Target::Machine(machine)) = self.target(&task) {
                let command = format!("ssh {}", transport::quote(&machine.target));
                server::run_in_pane(&target, &tab.pane_id, &command);
            }
        }
        let tab_id = tab.tab_id.clone();
        if let Some(data) = self.data.get_mut(&id) {
            data.tabs.push(tab);
        }
        self.select_tab(id, &tab_id);
    }

    /// Show a tab in its window and on the **Server** (`tab focus`, plan decision 2).
    fn select_tab(&mut self, id: WinId, tab_id: &str) {
        let Some(data) = self.data.get_mut(&id) else {
            return;
        };
        if data.workspace.active_tab_id == tab_id {
            return;
        }
        data.workspace.active_tab_id = tab_id.to_string();
        let key = data.target.clone();
        if let Some(target) = self.target(&key) {
            server::focus_tab(target, tab_id);
        }
        if let Err(err) = self.show_panes(id) {
            self.notice = Some(format!("open tab failed: {err}"));
        }
        self.refresh_title(id);
    }

    fn close_tab(&mut self, id: WinId, tab_id: &str) {
        let Some(data) = self.data.get_mut(&id) else {
            return;
        };
        if data.tabs.len() <= 1 {
            self.close_window_now(id);
            return;
        }
        let index = data.tabs.iter().position(|t| t.tab_id == tab_id);
        let Some(index) = index else {
            return;
        };
        data.tabs.remove(index);
        if data.workspace.active_tab_id == tab_id {
            let next = data.tabs[index.min(data.tabs.len() - 1)].tab_id.clone();
            data.workspace.active_tab_id = next;
        }
        let key = data.target.clone();
        if let Some(target) = self.target(&key) {
            server::close_tab(target, tab_id);
        }
        if let Err(err) = self.show_panes(id) {
            self.notice = Some(format!("open tab failed: {err}"));
        }
        self.refresh_title(id);
    }

    /// Reconcile one **Target**'s windows with its server state: new workspaces open, closed
    /// ones go, tabs and labels follow, streams that ended restart in place.
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
        for workspace in &snap.workspaces {
            let tabs: Vec<TabInfo> = snap
                .tabs
                .iter()
                .filter(|t| t.workspace_id == workspace.workspace_id)
                .cloned()
                .collect();
            if tabs.is_empty() {
                continue;
            }
            match self.win_id(key, &workspace.workspace_id) {
                Some(id) => {
                    if let Some(data) = self.data.get_mut(&id) {
                        data.workspace = workspace.clone();
                        data.tabs = tabs;
                    }
                    if let Err(err) = self.show_panes(id) {
                        tracing::warn!(%err, "desk: reattach failed");
                    }
                    self.refresh_title(id);
                }
                None => {
                    if let Err(err) = self.attach(key, workspace.clone(), tabs, None) {
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
                    && !snap
                        .workspaces
                        .iter()
                        .any(|w| w.workspace_id == data.workspace.workspace_id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.drop_window(id);
        }
        if key == LOCAL_KEY {
            let open: Vec<String> = self
                .data
                .values()
                .filter(|d| d.target == LOCAL_KEY)
                .map(|d| d.workspace.workspace_id.clone())
                .collect();
            self.ssh_workspaces.retain(|ws, _| open.contains(ws));
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

    /// **SSH Tier**: a new local window named after the **Machine**, running `ssh <target>`,
    /// in the **Machine**'s **Task** (plan decision 3A of B15).
    fn open_ssh(&mut self, key: &str) {
        let Some(Target::Machine(machine)) = self.target(key).cloned() else {
            return;
        };
        let opened = server::create_workspace(&Target::Local, Some(&machine.label)).and_then(
            |(workspace, tab)| {
                self.ssh_workspaces
                    .insert(workspace.workspace_id.clone(), key.to_string());
                let pane = tab.pane_id.clone();
                self.attach(LOCAL_KEY, workspace, vec![tab], None)
                    .map(|_| pane)
            },
        );
        match opened {
            Ok(pane) => {
                let command = format!("ssh {}", transport::quote(&machine.target));
                server::run_in_pane(&Target::Local, &pane, &command);
            }
            Err(err) => self.notice = Some(format!("ssh window failed: {err}")),
        }
    }

    /// Windows in the **Task** of `key`, in stacking order.
    fn task_windows(&self, key: &str) -> Vec<WinId> {
        self.desk
            .windows
            .iter()
            .map(|w| w.id)
            .filter(|id| self.data.get(id).is_some_and(|d| d.task == key))
            .collect()
    }

    /// **Taskbar** buttons: Local, then each connected **Machine** or one with windows.
    fn task_keys(&self) -> Vec<String> {
        self.targets
            .iter()
            .map(|t| t.target.key())
            .filter(|key| {
                *key == LOCAL_KEY
                    || self
                        .targets
                        .iter()
                        .any(|t| t.target.key() == *key && t.connected)
                    || self.data.values().any(|d| d.task == *key)
            })
            .map(str::to_string)
            .collect()
    }

    fn label(&self, key: &str) -> String {
        self.target(key).map_or(key, Target::label).to_string()
    }

    /// Rows of the **Machines Folder**, in `targets` order.
    fn folder_rows(&self) -> Vec<folder::Row> {
        self.targets
            .iter()
            .map(|t| {
                let windows = self.task_windows(t.target.key()).len();
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
                for id in self.task_windows(key) {
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

    /// Stop following a **Machine** and close its windows; its workspaces stay on its
    /// **Server**.
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
        self.saved
            .windows
            .retain(|s| !captured.windows.iter().any(|c| c.workspace == s.workspace));
        self.saved.windows.extend(captured.windows);
        for id in ids {
            self.drop_window(id);
        }
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
                state::key_machine(&s.workspace).is_some_and(|m| !connected.contains(&m))
                    && !out.windows.iter().any(|o| o.workspace == s.workspace)
            })
            .cloned()
            .collect();
        out.windows.splice(0..0, kept);
        out.taskbar = self.bar;
        out.machines = connected
            .into_iter()
            .filter(|key| *key != LOCAL_KEY)
            .map(str::to_string)
            .collect();
        out.ssh_workspaces = self.ssh_workspaces.clone();
        out
    }

    /// The workspace name; a **Machine**'s windows carry its name, an unreachable one `⚠`.
    fn refresh_title(&mut self, id: WinId) {
        let Some(data) = self.data.get(&id) else {
            return;
        };
        let mut title = data.workspace.label.clone();
        if data.target != LOCAL_KEY {
            title = format!("{}: {title}", self.label(&data.target));
        }
        let offline = data.panes.iter().any(|p| p.session.is_none())
            || matches!(self.link(&data.target), Link::Offline | Link::Auth);
        if offline {
            title = format!("⚠ {title}");
        }
        if let Some(w) = self.desk.get_mut(id) {
            w.title = title;
        }
    }

    /// Remove a window from the desk; its workspace stays on the server.
    fn drop_window(&mut self, id: WinId) {
        self.desk.close(id);
        if let Some(mut data) = self.data.remove(&id) {
            data.release();
        }
    }

    /// `[×]`: closes the workspace, after asking when it has more than one tab.
    fn close_window(&mut self, id: WinId) {
        let tabs = self.data.get(&id).map_or(0, |d| d.tabs.len());
        if tabs > 1 {
            let label = self
                .data
                .get(&id)
                .map(|d| d.workspace.label.clone())
                .unwrap_or_default();
            self.dialog = Some(Dialog::confirm(
                "Close window",
                format!("Close {label} and its {tabs} tabs?"),
                "Close",
                DialogTarget::CloseWindow(id),
            ));
            return;
        }
        self.close_window_now(id);
    }

    fn close_window_now(&mut self, id: WinId) {
        self.desk.close(id);
        if self.folder.is_some_and(|f| f.win == id) {
            self.folder = None;
        }
        if let Some(mut data) = self.data.remove(&id) {
            data.release();
            self.ssh_workspaces.remove(&data.workspace.workspace_id);
            if let Some(target) = self.target(&data.target) {
                server::close_workspace(target, &data.workspace.workspace_id);
            }
        }
    }

    /// Send a resize for every pane whose size changed since the last one sent.
    fn sync_sizes(&mut self) {
        let ids: Vec<WinId> = self.desk.windows.iter().map(|w| w.id).collect();
        for id in ids {
            let rects = self.pane_rects(id);
            let Some(data) = self.data.get_mut(&id) else {
                continue;
            };
            for (pane, rect) in data.panes.iter_mut().zip(rects) {
                let size = (rect.width, rect.height);
                if size.0 == 0 || size.1 == 0 || pane.sent_size == size {
                    continue;
                }
                if let Some(session) = pane.session.as_mut() {
                    pane.sent_size = size;
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
                serial,
                width,
                height,
                bytes,
            } => {
                self.stats.frames += 1;
                self.stats.frame_bytes += bytes.len() as u64;
                if let Some(pane) = self
                    .data
                    .get_mut(&win)
                    .and_then(|d| d.pane_by_serial(serial))
                {
                    pane.mirror.feed(width, height, &bytes);
                }
            }
            // The stream ended: keep the window until the next snapshot says whether its
            // workspace is gone (drop) or still there (restart in place).
            DeskEvent::Closed(win, serial) => {
                let ended = self
                    .data
                    .get_mut(&win)
                    .and_then(|d| d.pane_by_serial(serial))
                    .and_then(|p| p.session.take());
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

    /// The stream of a window's pane.
    fn session(&mut self, id: WinId, pane: usize) -> Option<&mut Session> {
        self.data
            .get_mut(&id)?
            .panes
            .get_mut(pane)?
            .session
            .as_mut()
    }

    /// Keys go to the focused window's focused pane.
    fn forward_input(&mut self, bytes: &[u8]) {
        let Some(id) = self.desk.focus else {
            return;
        };
        let pane = self.data.get(&id).map_or(0, WinData::focused_pane);
        if let Some(session) = self.session(id, pane) {
            session.input(bytes);
        }
    }

    /// Focus a pane of the shown tab, here and on the **Server** (`pane.focus`).
    fn focus_pane(&mut self, id: WinId, pane: usize) {
        let Some(data) = self.data.get_mut(&id) else {
            return;
        };
        if data.focused_pane() == pane {
            return;
        }
        let active = data.workspace.active_tab_id.clone();
        let Some(tab) = data.tabs.iter_mut().find(|t| t.tab_id == active) else {
            return;
        };
        let Some(pane_id) = tab.panes.get(pane).map(|p| p.pane_id.clone()) else {
            return;
        };
        tab.focused_pane_id = pane_id.clone();
        let key = data.target.clone();
        if let Some(target) = self.target(&key) {
            server::focus_pane(target, &pane_id);
        }
    }

    fn content_point(&self, id: WinId, pane: usize, column: u16, row: u16) -> Option<(u16, u16)> {
        let inner = *self.pane_rects(id).get(pane)?;
        let x = column
            .checked_sub(inner.x)?
            .min(inner.width.saturating_sub(1));
        let y = row
            .checked_sub(inner.y)?
            .min(inner.height.saturating_sub(1));
        Some((x, y))
    }

    fn forward_mouse(
        &mut self,
        (id, pane): (WinId, usize),
        action: &str,
        button: &str,
        column: u16,
        row: u16,
    ) {
        let Some((x, y)) = self.content_point(id, pane, column, row) else {
            return;
        };
        if let Some(session) = self.session(id, pane) {
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
                    (Some(Hit::Content(id, pane)), button) => {
                        self.focus_pane(id, pane);
                        self.content_press = Some((id, pane));
                        self.forward_mouse((id, pane), "down", button_name(button), column, row);
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
                    (Some(Hit::Tab(id, index)), MouseButton::Left) => {
                        let tab = self.tab_id(id, index);
                        if let Some(tab) = tab {
                            self.select_tab(id, &tab);
                        }
                    }
                    (Some(Hit::Tab(id, index)), MouseButton::Right) => {
                        self.menu = self.tab_menu(id, index, (column, row));
                    }
                    (Some(Hit::TabClose(id, index)), MouseButton::Left) => {
                        let tab = self.tab_id(id, index);
                        if let Some(tab) = tab {
                            self.close_tab(id, &tab);
                        }
                    }
                    (Some(Hit::NewTab(id)), MouseButton::Left) => self.new_tab(id),
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
                        let x = self.bar.bar(self.screen).x;
                        self.menu = Some(self.bar_popup(start_menu(), (x, row)));
                    }
                    (Some(Hit::Task(index)), button) => {
                        let at = (column, row);
                        self.menu = match button {
                            MouseButton::Right => self.task_menu(index, at),
                            _ => self.task_windows_menu(index, at),
                        }
                        .map(|m| self.bar_popup(m, at));
                    }
                    (Some(Hit::Bar), MouseButton::Left) => {
                        self.bar_drag = Some(BarDrag::Move(None))
                    }
                    (Some(Hit::Bar), MouseButton::Right) => {
                        self.menu = self.taskbar_menu((column, row));
                    }
                    (Some(Hit::Grip), MouseButton::Left) => self.bar_drag = Some(BarDrag::Resize),
                    _ => {}
                }
            }
            MouseEventKind::Drag(button) => {
                if let Some(bar_drag) = self.bar_drag {
                    match bar_drag {
                        BarDrag::Move(_) => {
                            let edge = taskbar::nearest_edge((column, row), self.screen);
                            self.bar_drag = Some(BarDrag::Move(Some(edge)));
                        }
                        BarDrag::Resize => {
                            self.bar = self.bar.resized((column, row), self.screen);
                            self.desk.refit(self.desk_rect());
                        }
                    }
                } else if let Some(mut drag) = self.drag {
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
                } else if let Some(press) = self.content_press {
                    self.forward_mouse(press, "drag", button_name(button), column, row);
                }
            }
            MouseEventKind::Up(button) => {
                if let Some(bar_drag) = self.bar_drag.take() {
                    let place = match bar_drag {
                        BarDrag::Move(Some(edge)) => self.bar.moved(edge, self.screen),
                        _ => self.bar,
                    };
                    self.set_bar(place);
                } else if let Some(drag) = self.drag.take() {
                    if let (Some(zone), Some(w)) = (drag.zone, self.desk.get_mut(drag.id)) {
                        w.rect = zone;
                        if zone == drag.desk {
                            w.state = WinState::Maximized {
                                restore: drag.original,
                            };
                        }
                    }
                    self.sync_sizes();
                } else if let Some(press) = self.content_press.take() {
                    self.forward_mouse(press, "up", button_name(button), column, row);
                }
            }
            MouseEventKind::Moved => {
                if let Some(Hit::Content(id, pane)) = self.hits.hit(column, row) {
                    if Some(id) == self.desk.focus {
                        self.forward_mouse((id, pane), "move", "left", column, row);
                    }
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if let Some(Hit::Content(id, pane)) = self.hits.hit(column, row) {
                    let up = mouse.kind == MouseEventKind::ScrollUp;
                    if let Some((x, y)) = self.content_point(id, pane, column, row) {
                        if let Some(session) = self.session(id, pane) {
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
        let desk = self.bar.desk(screen);
        let mut hits = HitMap::default();
        frame
            .buffer_mut()
            .set_style(desk, Style::default().bg(DESK_BG));
        hits.push(desk, Hit::Desktop);
        // The grip: a thin line along the bar, under the windows.
        let grip = self.bar.grip(screen);
        let symbol = match self.bar.edge {
            Edge::Bottom => "▁",
            Edge::Top => "▔",
            Edge::Left => "▏",
            Edge::Right => "▕",
        };
        for position in grip.positions() {
            frame.buffer_mut()[position]
                .set_symbol(symbol)
                .set_fg(BAR_BG);
        }
        hits.push(grip, Hit::Grip);
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
            let mut tab_hits = Vec::new();
            if let Some(data) = self.data.get(&w.id) {
                let area = content_area(inner);
                let rects = self.pane_rects(w.id);
                let focused_pane = data.focused_pane();
                draw_dividers(&rects, area, frame.buffer_mut(), border);
                for (index, (pane, rect)) in data.panes.iter().zip(&rects).enumerate() {
                    pane.mirror
                        .render(frame, *rect, focused && index == focused_pane);
                    hits.push(*rect, Hit::Content(w.id, index));
                }
                render_tabs(
                    data,
                    w.id,
                    focused,
                    tab_strip(inner),
                    frame.buffer_mut(),
                    |r, h| {
                        tab_hits.push((r, h));
                    },
                );
            } else {
                hits.push(inner, Hit::Content(w.id, 0));
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
            for (rect, hit) in tab_hits {
                hits.push(rect, hit);
            }
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

        // **Taskbar** (B21): on any edge; a dragged bar shows where it would land.
        let bar = self.bar.bar(screen);
        if !bar.is_empty() {
            let buf = frame.buffer_mut();
            buf.set_style(bar, Style::default().bg(BAR_BG).fg(Color::White));
            hits.push(bar, Hit::Bar);
            let current = self.current_task();
            let keys = self.task_keys();
            let texts: Vec<String> = keys
                .iter()
                .map(|key| taskbar::task_text(&self.label(key), self.task_windows(key).len()))
                .collect();
            let layout = taskbar::layout(bar, self.bar.edge, texts);
            let start_style = if self.menu.as_ref().is_some_and(|m| m.title == START_TITLE) {
                Style::default().bg(FOCUS_BORDER).fg(Color::Black)
            } else {
                Style::default()
                    .bg(Color::Indexed(25))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            };
            buf.set_stringn(
                layout.start.x,
                layout.start.y,
                taskbar::START,
                usize::from(layout.start.width),
                start_style,
            );
            hits.push(layout.start, Hit::Start);
            for (index, button) in layout.buttons.iter().enumerate() {
                let active = keys.get(index) == Some(&current);
                let style = if active {
                    Style::default().bg(FOCUS_BORDER).fg(Color::Black)
                } else {
                    Style::default().bg(Color::Indexed(239)).fg(Color::White)
                };
                buf.set_string(button.rect.x, button.rect.y, &button.text, style);
                hits.push(button.rect, Hit::Task(index));
            }
            if let Some(hint_rect) = layout.hint {
                let hint = match (&self.notice, self.prefix_armed) {
                    (_, true) => {
                        "desk: n new · w close · z min · m max · t tile · Tab next · q quit"
                            .to_string()
                    }
                    (Some(notice), false) => notice.clone(),
                    (None, false) => format!(
                        "{}{} windows · Ctrl+\\ desk keys",
                        self.link_summary(),
                        self.desk.windows.len()
                    ),
                };
                buf.set_stringn(
                    hint_rect.x + 1,
                    hint_rect.y,
                    hint,
                    usize::from(hint_rect.width.saturating_sub(1)),
                    Style::default().bg(BAR_BG).fg(Color::Gray),
                );
            }
        }
        if let Some(BarDrag::Move(Some(edge))) = self.bar_drag {
            let target = self.bar.moved(edge, screen).bar(screen);
            frame
                .buffer_mut()
                .set_style(target, Style::default().bg(Color::Indexed(31)));
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
        let mut items = Vec::new();
        if self.data.contains_key(&id) {
            items.push(MenuItem::new("Rename…", "", Action::RenameWindow(id)));
            items.push(MenuItem::new("New tab", "", Action::NewTab(id)));
        }
        items.extend([
            MenuItem::new("Minimize", "^\\ z", Action::Minimize(id)),
            MenuItem::new(maximize, "^\\ m", Action::ToggleMaximize(id)),
            MenuItem::new("Close", "^\\ w", Action::Close(id)),
        ]);
        Menu::open(w.title.clone(), at, items)
    }

    fn tab_id(&self, id: WinId, index: usize) -> Option<String> {
        Some(self.data.get(&id)?.tabs.get(index)?.tab_id.clone())
    }

    fn tab_menu(&self, id: WinId, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let tab = self.data.get(&id)?.tabs.get(index)?;
        Menu::open(
            tab.label.clone(),
            at,
            vec![
                MenuItem::new("Rename tab…", "", Action::RenameTab(id, tab.tab_id.clone())),
                MenuItem::new("Close tab", "", Action::CloseTab(id, tab.tab_id.clone())),
            ],
        )
    }

    /// Left click on a Task button: its windows, minimized ones marked.
    fn task_windows_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let key = self.task_keys().into_iter().nth(index)?;
        let items = self
            .task_windows(&key)
            .into_iter()
            .filter_map(|id| self.desk.get(id))
            .map(|w| {
                let mark = if w.minimized { "▁ " } else { "  " };
                MenuItem::new(format!("{mark}{}", w.title), "", Action::Raise(w.id))
            })
            .collect();
        Menu::open(self.label(&key), at, items).map(Menu::above)
    }

    fn task_menu(&self, index: usize, at: (u16, u16)) -> Option<Menu<Action>> {
        let key = self.task_keys().into_iter().nth(index)?;
        let mut items = vec![
            MenuItem::new("New window", "", Action::NewWindowOn(key.clone())),
            MenuItem::new("Minimize all", "", Action::MinimizeTask(key.clone())),
            MenuItem::new("Restore all", "", Action::RestoreTask(key.clone())),
        ];
        if let Some(state) = self.targets.iter().find(|t| t.target.key() == key) {
            match (&state.target, state.connected) {
                (Target::Local, _) => {}
                (Target::Machine(_), true) => {
                    items.push(MenuItem::new(
                        "Disconnect",
                        "",
                        Action::Disconnect(key.clone()),
                    ));
                }
                (Target::Machine(_), false) => {
                    items.push(MenuItem::new("Connect", "", Action::Connect(key.clone())));
                }
            }
        }
        Menu::open(self.label(&key), at, items).map(Menu::above)
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
                        &data.workspace.label,
                        "Rename",
                        DialogTarget::RenameWindow(id),
                    ));
                }
            }
            Action::NewTab(id) => self.new_tab(id),
            Action::RenameTab(id, tab_id) => {
                let label = self
                    .data
                    .get(&id)
                    .and_then(|d| d.tabs.iter().find(|t| t.tab_id == tab_id))
                    .map(|t| t.label.clone());
                if let Some(label) = label {
                    self.dialog = Some(Dialog::input(
                        "Rename tab",
                        &label,
                        "Rename",
                        DialogTarget::RenameTab(id, tab_id),
                    ));
                }
            }
            Action::CloseTab(id, tab_id) => self.close_tab(id, &tab_id),
            Action::NewWindow => self.new_window(),
            Action::NewWindowOn(key) => self.new_window_on(&key),
            Action::TileAll => {
                self.desk.tile_all(desk);
                self.sync_sizes();
            }
            Action::Quit => self.quit = true,
            Action::TaskbarEdge(edge) => self.set_bar(self.bar.moved(edge, self.screen)),
            Action::TaskbarSize(delta) => {
                let size = self.bar.size.saturating_add_signed(i16::from(delta));
                self.set_bar(self.bar.sized(size, self.screen));
            }
            Action::OpenFolder => self.open_folder(),
            Action::OpenMachine(key) => self.open_machine(&key),
            Action::Connect(key) => {
                self.ssh_on_tier.push(key.clone());
                self.connect(&key);
            }
            Action::Disconnect(key) => self.disconnect(&key),
            Action::OpenSsh(key) => self.open_ssh(&key),
            Action::MinimizeTask(key) => {
                for id in self.task_windows(&key) {
                    self.desk.minimize(id);
                }
            }
            Action::RestoreTask(key) => {
                for id in self.task_windows(&key) {
                    self.desk.raise(id);
                }
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
                data.workspace.label = text.to_string();
                let (key, ws) = (data.target.clone(), data.workspace.workspace_id.clone());
                if let Some(target) = self.target(&key) {
                    server::rename_workspace(target, &ws, text);
                }
                self.refresh_title(id);
            }
            DialogTarget::RenameTab(id, tab_id) if !text.is_empty() => {
                let Some(data) = self.data.get_mut(&id) else {
                    return;
                };
                if let Some(tab) = data.tabs.iter_mut().find(|t| t.tab_id == tab_id) {
                    tab.label = text.to_string();
                }
                let key = data.target.clone();
                if let Some(target) = self.target(&key) {
                    server::rename_tab(target, &tab_id, text);
                }
            }
            DialogTarget::CloseWindow(id) => self.close_window_now(id),
            _ => {}
        }
    }
}

const START_TITLE: &str = "Start";

/// Open a popup from the **Taskbar** at the pointer, on the desk side of the bar.
fn bar_popup<A: Clone>(
    place: Place,
    screen: Rect,
    mut menu: Menu<A>,
    (column, row): (u16, u16),
) -> Menu<A> {
    let bar = place.bar(screen);
    let (at, above) = match place.edge {
        Edge::Bottom => ((column, row), true),
        Edge::Top => ((column, row.saturating_add(1)), false),
        Edge::Left => ((bar.right(), row), false),
        // Clamped into the desk, so it opens left of the bar.
        Edge::Right => ((column, row), false),
    };
    menu.at = at;
    menu.above = above;
    menu
}

/// The **Start Menu**; [`bar_popup`] places it beside the Start button.
fn start_menu() -> Menu<Action> {
    let items = vec![
        MenuItem::new("Machines", "", Action::OpenFolder),
        MenuItem::new("New window", "^\\ n", Action::NewWindow),
        MenuItem::new("Tile all", "^\\ t", Action::TileAll),
        MenuItem::new("Quit desk", "^\\ q", Action::Quit),
    ];
    Menu {
        title: START_TITLE.to_string(),
        at: (0, 0),
        above: true,
        items,
        highlighted: 0,
    }
}

/// The tab strip: the first row inside a **Terminal Window**.
fn tab_strip(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y, inner.width, inner.height.min(1))
}

/// Below the tab strip: where the shown tab's panes go.
fn content_area(inner: Rect) -> Rect {
    Rect::new(
        inner.x,
        inner.y.saturating_add(1).min(inner.bottom()),
        inner.width,
        inner.height.saturating_sub(1),
    )
}

/// A tab's pane rects in `area`: the **Server** layout scaled to fit, one cell left between
/// neighbours for a divider.
fn layout_rects(tab: &TabInfo, area: Rect) -> Vec<Rect> {
    let (aw, ah) = (u32::from(tab.area.0.max(1)), u32::from(tab.area.1.max(1)));
    let scale = |v: u16, total: u32, size: u16| {
        let scaled = (u32::from(v) * u32::from(size) + total / 2) / total;
        u16::try_from(scaled).unwrap_or(size).min(size)
    };
    tab.panes
        .iter()
        .map(|slot| {
            let r = slot.rect;
            let left = area.x + scale(r.x, aw, area.width);
            let top = area.y + scale(r.y, ah, area.height);
            let mut right = area.x + scale(r.x.saturating_add(r.width), aw, area.width);
            let mut bottom = area.y + scale(r.y.saturating_add(r.height), ah, area.height);
            if right < area.right() && right > left + 1 {
                right -= 1;
            }
            if bottom < area.bottom() && bottom > top + 1 {
                bottom -= 1;
            }
            Rect::new(
                left,
                top,
                right.saturating_sub(left),
                bottom.saturating_sub(top),
            )
        })
        .collect()
}

/// The cells between panes: `│` beside a pane, `─` below it.
fn draw_dividers(rects: &[Rect], area: Rect, buf: &mut ratatui::buffer::Buffer, style: Style) {
    for r in rects {
        if r.right() < area.right() {
            for y in r.y..r.bottom().saturating_add(1).min(area.bottom()) {
                buf[(r.right(), y)].set_symbol("│").set_style(style);
            }
        }
        if r.bottom() < area.bottom() {
            for x in r.x..r.right().min(area.right()) {
                buf[(x, r.bottom())].set_symbol("─").set_style(style);
            }
        }
    }
}

/// ` label × ` per tab, then ` + `; tabs that do not fit are left out.
fn render_tabs(
    data: &WinData,
    id: WinId,
    focused: bool,
    strip: Rect,
    buf: &mut ratatui::buffer::Buffer,
    mut push: impl FnMut(Rect, Hit),
) {
    if strip.is_empty() {
        return;
    }
    buf.set_style(strip, Style::default().bg(BAR_BG).fg(Color::Gray));
    let active = data.active_index();
    let mut x = strip.x;
    let end = strip.right();
    for (index, tab) in data.tabs.iter().enumerate() {
        let label = format!(" {} ", tab.label);
        let width = super::text_width(&label);
        if x.saturating_add(width).saturating_add(5) > end {
            break;
        }
        let style = match (index == active, focused) {
            (true, true) => Style::default().bg(FOCUS_BORDER).fg(Color::Black),
            (true, false) => Style::default().bg(BLUR_BORDER).fg(Color::Black),
            (false, _) => Style::default().bg(Color::Indexed(239)).fg(Color::White),
        };
        buf.set_string(x, strip.y, &label, style);
        push(Rect::new(x, strip.y, width, 1), Hit::Tab(id, index));
        x += width;
        buf.set_string(x, strip.y, "×", style.fg(Color::LightRed));
        push(Rect::new(x, strip.y, 1, 1), Hit::TabClose(id, index));
        x += 2;
    }
    if x.saturating_add(3) <= end {
        buf.set_string(
            x,
            strip.y,
            " + ",
            Style::default().bg(Color::Indexed(239)).fg(Color::White),
        );
        push(Rect::new(x, strip.y, 3, 1), Hit::NewTab(id));
    }
}

fn hit_window(hit: Option<Hit>) -> Option<WinId> {
    match hit? {
        Hit::Content(id, _)
        | Hit::Title(id)
        | Hit::Handle(id, _)
        | Hit::Minimize(id)
        | Hit::Maximize(id)
        | Hit::Close(id)
        | Hit::Tab(id, _)
        | Hit::TabClose(id, _)
        | Hit::NewTab(id) => Some(id),
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
    let state_path = state::path();
    let saved = state::load(&state_path);
    let mut app = App {
        targets,
        saved: state::DeskState::default(),
        ssh_workspaces: saved.ssh_workspaces.clone(),
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
        last_row_click: None,
        ssh_on_tier: Vec::new(),
        bar: Place::default(),
        bar_drag: None,
    };
    let reattached = snap.workspaces.len();
    app.apply(LOCAL_KEY, snap);
    app.set_bar(saved.taskbar);
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
    let result = event_loop(&mut app, &mut terminal, &rx, &state_path);
    state::save(&state_path, &app.capture());
    for (_, mut data) in app.data.drain() {
        data.release();
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
    state_path: &std::path::Path,
) -> io::Result<()> {
    let mut framer = RawInputByteFramer::for_host_input();
    let mut autosave = Autosave::default();
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
        autosave.tick(app, state_path);
    }
    Ok(())
}

/// Writes `desk.json` once the layout has stayed unchanged for [`SAVE_AFTER`] (B23), so a
/// killed desk keeps its last layout. Checked at most every [`SAVE_CHECK`].
#[derive(Default)]
struct Autosave {
    written: Option<state::DeskState>,
    pending: Option<(state::DeskState, Instant)>,
    checked: Option<Instant>,
}

const SAVE_AFTER: Duration = Duration::from_secs(1);
const SAVE_CHECK: Duration = Duration::from_millis(250);

impl Autosave {
    fn tick(&mut self, app: &App, path: &std::path::Path) {
        if self.checked.is_some_and(|at| at.elapsed() < SAVE_CHECK) {
            return;
        }
        self.checked = Some(Instant::now());
        let now = app.capture();
        if self.written.as_ref() == Some(&now) {
            self.pending = None;
            return;
        }
        match &self.pending {
            Some((pending, since)) if *pending == now => {
                if since.elapsed() >= SAVE_AFTER {
                    state::save(path, &now);
                    self.written = Some(now);
                    self.pending = None;
                }
            }
            _ => self.pending = Some((now, Instant::now())),
        }
    }
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
        let menu = start_menu();
        let actions: Vec<Action> = menu.items.iter().map(|i| i.action.clone()).collect();
        assert_eq!(
            actions,
            [
                Action::OpenFolder,
                Action::NewWindow,
                Action::TileAll,
                Action::Quit
            ]
        );
        let screen = Rect::new(0, 0, 120, 36);
        for edge in [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right] {
            let place = Place::default().moved(edge, screen);
            let (bar, desk) = (place.bar(screen), place.desk(screen));
            let start = taskbar::layout(bar, edge, Vec::new()).start;
            let menu = bar_popup(place, screen, start_menu(), (start.x, start.y));
            let rect = menu.rect(desk).unwrap();
            assert_eq!(rect.intersection(desk), rect, "{edge:?}");
            // Next to the Start button.
            let gap = match edge {
                Edge::Bottom => bar.y - rect.bottom(),
                Edge::Top => rect.y - bar.bottom(),
                Edge::Left => rect.x - bar.right(),
                Edge::Right => bar.x - rect.right(),
            };
            assert_eq!(gap, 0, "{edge:?}");
        }
    }

    fn tab(id: &str, label: &str) -> TabInfo {
        TabInfo {
            tab_id: id.into(),
            workspace_id: "w1".into(),
            label: label.into(),
            pane_id: format!("{id}-p"),
            terminal_id: format!("{id}-t"),
            panes: Vec::new(),
            area: (1, 1),
            focused_pane_id: String::new(),
        }
    }

    #[test]
    fn tab_strip_lists_tabs_with_close_marks_then_new_tab() {
        let data = WinData {
            target: LOCAL_KEY.into(),
            task: LOCAL_KEY.into(),
            workspace: WorkspaceInfo {
                workspace_id: "w1".into(),
                label: "api".into(),
                active_tab_id: "w1:t2".into(),
            },
            tabs: vec![tab("w1:t1", "one"), tab("w1:t2", "two")],
            attached_at: Instant::now(),
            panes: Vec::new(),
        };
        assert_eq!(data.active_index(), 1);
        let strip = Rect::new(2, 3, 40, 1);
        let mut buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 50, 5));
        let mut hits = Vec::new();
        render_tabs(&data, 7, true, strip, &mut buf, |r, h| hits.push((r.x, h)));
        assert_eq!(
            hits,
            [
                (2, Hit::Tab(7, 0)),
                (7, Hit::TabClose(7, 0)),
                (9, Hit::Tab(7, 1)),
                (14, Hit::TabClose(7, 1)),
                (16, Hit::NewTab(7)),
            ]
        );
        // Tabs that do not fit are left out; `+` stays when there is room.
        let mut hits = Vec::new();
        render_tabs(&data, 7, true, Rect::new(0, 0, 12, 1), &mut buf, |_, h| {
            hits.push(h)
        });
        assert_eq!(hits, [Hit::Tab(7, 0), Hit::TabClose(7, 0), Hit::NewTab(7)]);
        assert_eq!(content_area(Rect::new(1, 1, 10, 5)), Rect::new(1, 2, 10, 4));
    }

    #[test]
    fn layouts_scale_into_the_window_with_dividers() {
        let slot = |id: &str, x, y, w, h| super::server::PaneSlot {
            pane_id: id.into(),
            terminal_id: id.into(),
            rect: Rect::new(x, y, w, h),
        };
        let mut t = tab("w1:t1", "one");
        t.area = (120, 40);
        t.panes = vec![
            slot("a", 0, 0, 60, 12),
            slot("b", 0, 12, 60, 28),
            slot("c", 60, 0, 60, 40),
        ];
        let area = Rect::new(10, 5, 60, 20);
        let rects = layout_rects(&t, area);
        assert_eq!(rects[0], Rect::new(10, 5, 29, 5));
        assert_eq!(rects[1], Rect::new(10, 11, 29, 14));
        assert_eq!(rects[2], Rect::new(40, 5, 30, 20));
        // Panes never overlap and stay inside the area.
        for (i, a) in rects.iter().enumerate() {
            assert_eq!(a.intersection(area), *a);
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "{a:?} {b:?}");
            }
        }
    }
}
