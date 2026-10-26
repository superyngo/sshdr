//! app.rs glue for **Files** windows (spec `2026-10-25-desk-files.md` F1): opening, routing
//! events into `files`, carrying out its requests, and saving them in `desk.json`.

use std::path::Path;

use crossterm::event::MouseButton;
use ratatui::{buffer::Buffer, layout::Rect};

use super::{Action, App, Hit, Link, Target, LOCAL_KEY};
use crate::cli::desk::files::{
    self, listing, Files, FilesHit, Location, MachineRow, Request, Views,
};
use crate::cli::desk::menu::{Menu, MenuItem};
use crate::cli::desk::server;
use crate::cli::desk::state::{self, DeskState};
use crate::cli::desk::wm::WinId;

impl App {
    /// Rows of the `Machines` root, in `targets` order.
    pub(super) fn machine_rows(&self) -> Vec<MachineRow> {
        self.targets
            .iter()
            .map(|t| {
                let key = t.target.key();
                let windows = self.task_windows(key).len();
                let (target, status) = match &t.target {
                    Target::Local => ("this computer".to_string(), "Local".to_string()),
                    Target::Machine(m) => (m.target.clone(), t.link.text().to_string()),
                };
                let on = Self::is_on(t);
                let status = if on && windows > 0 {
                    format!("{status} · {windows}")
                } else {
                    status
                };
                MachineRow {
                    key: key.to_string(),
                    name: t.target.label().to_string(),
                    target,
                    status,
                    alert: matches!(t.link, Link::Auth | Link::Offline),
                    on,
                }
            })
            .collect()
    }

    /// A new **Files** window at `location`, with the saved defaults unless `views` is given.
    pub(super) fn open_files(&mut self, location: Location, views: Option<Views>) -> WinId {
        let win = self
            .desk
            .open(files::TASK_LABEL.to_string(), files::SIZE, self.desk_rect());
        let f = Files::new(win, location, views.unwrap_or(self.files_views));
        self.files.insert(win, f);
        self.files_request(win, Request::List);
        win
    }

    /// Start Menu Machines: raise a window showing the `Machines` root, or open one.
    pub(super) fn open_machines(&mut self) {
        let shown = self.files.values().find(|f| f.at_machines()).map(|f| f.win);
        match shown {
            Some(win) => self.desk.raise(win),
            None => {
                self.open_files(Location::Machines, None);
            }
        }
    }

    /// **Files** windows in stacking order.
    pub(super) fn files_windows(&self) -> Vec<WinId> {
        self.desk
            .windows
            .iter()
            .map(|w| w.id)
            .filter(|id| self.files.contains_key(id))
            .collect()
    }

    pub(super) fn files_key(&mut self, win: WinId, chunk: &[u8]) {
        let rows = self.machine_rows();
        let request = self.files.get_mut(&win).and_then(|f| f.key(chunk, &rows));
        if let Some(request) = request {
            self.files_request(win, request);
        }
        self.files_title(win);
    }

    pub(super) fn files_click(
        &mut self,
        win: WinId,
        hit: FilesHit,
        button: MouseButton,
        at: (u16, u16),
    ) {
        if let FilesHit::Bar(axis) = hit {
            self.files_bar_press(win, axis, at);
            return;
        }
        let rows = self.machine_rows();
        let right = button == MouseButton::Right;
        let request = self
            .files
            .get_mut(&win)
            .and_then(|f| f.click(hit, right, at, &rows));
        if let Some(request) = request {
            self.files_request(win, request);
        }
        self.files_title(win);
    }

    pub(super) fn files_listed(&mut self, win: WinId, listing: files::Listing) {
        if let Some(f) = self.files.get_mut(&win) {
            f.listed(listing);
            f.keep_visible();
        }
    }

    /// Idle tick: re-list folders whose modification time changed. True when one was started.
    pub(super) fn poll_files(&mut self) -> bool {
        let stale: Vec<WinId> = self
            .files
            .values()
            .filter(|f| !f.loading)
            .filter_map(|f| {
                let (machine, path) = f.folder()?;
                (machine == LOCAL_KEY && listing::modified(path) != f.stamp).then_some(f.win)
            })
            .collect();
        for &win in &stale {
            self.files_request(win, Request::List);
        }
        !stale.is_empty()
    }

    fn files_title(&mut self, win: WinId) {
        let Some(title) = self.files.get(&win).map(|f| f.title(|m| self.label(m))) else {
            return;
        };
        if let Some(w) = self.desk.get_mut(win) {
            w.title = title;
        }
    }

    pub(super) fn files_request(&mut self, win: WinId, request: Request) {
        match request {
            Request::List => {
                if let Some((generation, path)) =
                    self.files.get_mut(&win).and_then(Files::begin_listing)
                {
                    listing::spawn(win, generation, path, self.events.clone());
                }
                self.files_title(win);
            }
            Request::OpenMachine(key) => self.open_machine(&key),
            Request::Connect(key) => self.run_action(Action::Connect(key)),
            Request::Disconnect(key) => self.disconnect(&key),
            Request::NewWindow(key) => self.new_window_on(&key),
            Request::ShowWindows(key) => {
                for id in self.task_windows(&key) {
                    self.desk.raise(id);
                }
            }
            Request::MachineMenu(key, at) => self.menu = self.machine_menu(&key, at),
            Request::OpenTerminal(machine, path) => self.open_terminal_at(&machine, &path),
            Request::MakeDefault => {
                if let Some(f) = self.files.get(&win) {
                    if f.at_machines() {
                        self.files_views.machines = f.views.machines;
                    } else {
                        self.files_views.folders = f.views.folders;
                    }
                }
            }
        }
    }

    /// A new tab in the most recently used **Terminal Window** of the **Machine**, working in
    /// `path`; a new window when it has none.
    fn open_terminal_at(&mut self, machine: &str, path: &Path) {
        let cwd = path.to_string_lossy().into_owned();
        let recent = self.desk.windows.iter().rev().map(|w| w.id).find(|id| {
            self.data
                .get(id)
                .is_some_and(|d| d.target == machine && d.task == machine)
        });
        match recent {
            Some(id) => {
                self.new_tab(id, Some(&cwd));
                self.desk.raise(id);
            }
            None => {
                let Some(target) = self.target(machine).cloned() else {
                    return;
                };
                let created = server::create_workspace(&target, None, Some(&cwd))
                    .and_then(|(workspace, tab)| self.attach(machine, workspace, vec![tab], None));
                if let Err(err) = created {
                    self.notice = Some(format!("new window failed: {err}"));
                }
            }
        }
    }

    /// Right click on a **Machine**, in the `Machines` root.
    pub(super) fn machine_menu(&self, key: &str, at: (u16, u16)) -> Option<Menu<Action>> {
        let state = self.targets.iter().find(|t| t.target.key() == key)?;
        let key = key.to_string();
        let mut items = Vec::new();
        match (&state.target, Self::is_on(state)) {
            (Target::Local, _) => {
                items.push(MenuItem::new(
                    "Show windows",
                    "",
                    Action::OpenMachine(key.clone()),
                ));
                items.push(MenuItem::new("New window", "", Action::NewWindowOn(key)));
            }
            (Target::Machine(_), true) => {
                items.push(MenuItem::new(
                    "Show windows",
                    "",
                    Action::OpenMachine(key.clone()),
                ));
                items.push(MenuItem::new(
                    "New window",
                    "",
                    Action::NewWindowOn(key.clone()),
                ));
                items.push(MenuItem::new("Disconnect", "", Action::Disconnect(key)));
            }
            (Target::Machine(_), false) => {
                items.push(MenuItem::new("Connect", "", Action::Connect(key)));
            }
        }
        Menu::open(state.target.label().to_string(), at, items)
    }

    /// Draw a **Files** window's content; its hits go to `hits` in drawing order.
    pub(super) fn draw_files(
        &mut self,
        win: WinId,
        rows: &[MachineRow],
        inner: Rect,
        buf: &mut Buffer,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        let labels: Vec<(String, String)> = self
            .targets
            .iter()
            .map(|t| (t.target.key().to_string(), t.target.label().to_string()))
            .collect();
        let label = |key: &str| {
            labels
                .iter()
                .find(|(k, _)| k == key)
                .map_or_else(|| key.to_string(), |(_, l)| l.clone())
        };
        if let Some(f) = self.files.get_mut(&win) {
            files::render(f, rows, &label, inner, buf, &mut |rect, hit| {
                hits.push((rect, Hit::Files(win, hit)));
            });
        }
    }

    /// `desk.json`: open **Files** windows bottom to top, and the view defaults.
    pub(super) fn capture_files(&self, out: &mut DeskState) {
        out.files_views = self.files_views;
        out.files = self
            .desk
            .windows
            .iter()
            .filter_map(|w| {
                let f = self.files.get(&w.id)?;
                Some(state::SavedFiles {
                    window: state::saved(w, String::new()),
                    location: f.location.clone(),
                    views: f.views,
                    collapsed: f.collapsed,
                })
            })
            .collect();
    }

    /// Reopen saved **Files** windows above the **Terminal Windows**.
    pub(super) fn restore_files(&mut self, saved: &DeskState) {
        self.files_views = saved.files_views;
        let area = self.desk_rect();
        for entry in &saved.files {
            let win = self.open_files(entry.location.clone(), Some(entry.views));
            if let Some(w) = self.desk.get_mut(win) {
                state::apply_saved(w, &entry.window, area);
            }
            if let Some(f) = self.files.get_mut(&win) {
                f.collapsed = entry.collapsed;
            }
        }
        self.desk.focus = self.desk.top_visible();
    }
}
