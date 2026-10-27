//! **Files**: the **Desktop**'s file manager **App** (spec `docs/spec/2026-10-25-desk-files.md`,
//! ADR 0009). A window browses the `Machines` root or a folder on a **Machine**; app.rs routes
//! events here and carries out the [`Request`]s.

mod grid;
pub(super) mod listing;
pub(super) mod ops;
pub(super) mod remote;
mod remote_ops;
mod render;
mod select;
mod trash;

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use serde::{Deserialize, Serialize};

pub(super) use listing::{Entry, Listing};
pub(super) use render::render;

use ratatui::layout::Rect;

use super::scroll::{Axis, Extent};
use super::transport::LOCAL_KEY;
use super::wm::WinId;

/// Key of the **Files** **Task** on the **Taskbar**; not a profile id (those have no `@`).
pub(super) const TASK_KEY: &str = "@files";
pub(super) const TASK_LABEL: &str = "Files";
pub(super) const SIZE: (u16, u16) = (88, 24);
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Location {
    Machines,
    Folder { machine: String, path: PathBuf },
}

impl Location {
    pub fn home(machine: &str) -> Self {
        Self::Folder {
            machine: machine.to_string(),
            path: home_of(machine),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
    /// Kind in folders; Status at the `Machines` root.
    Kind,
}

impl SortKey {
    const ALL: [Self; 4] = [Self::Name, Self::Size, Self::Modified, Self::Kind];

    fn label(self, machines: bool) -> &'static str {
        match (self, machines) {
            (Self::Name, _) => "name",
            (Self::Size, _) => "size",
            (Self::Modified, _) => "modified",
            (Self::Kind, false) => "kind",
            (Self::Kind, true) => "status",
        }
    }
}

/// List (columns) or Grid (cells) (F2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Layout {
    #[default]
    List,
    Grid,
}

/// View and sort of one location kind; **Client** state in `desk.json` (ADR 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct View {
    pub layout: Layout,
    pub sort: SortKey,
    pub desc: bool,
    pub folders_first: bool,
    pub hidden: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            layout: Layout::List,
            sort: SortKey::Name,
            desc: false,
            folders_first: true,
            hidden: false,
        }
    }
}

/// One [`View`] per location kind: the defaults in `desk.json`, and each window's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Views {
    pub machines: View,
    pub folders: View,
}

impl Views {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A row of the `Machines` root, built by app.rs from its **Targets**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineRow {
    pub key: String,
    pub name: String,
    pub target: String,
    pub status: String,
    /// `!`-states draw in the warning color.
    pub alert: bool,
    /// Connected (or Local): Disconnect instead of Connect.
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RibbonTab {
    Home,
    View,
    Machine,
}

impl RibbonTab {
    const ALL: [Self; 3] = [Self::Home, Self::View, Self::Machine];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::View => "View",
            Self::Machine => "Machine",
        }
    }
}

/// A ribbon command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cmd {
    Open,
    OpenTerminal,
    Refresh,
    Layout(Layout),
    Cut,
    Copy,
    Paste,
    Rename,
    Trash,
    NewFolder,
    Delete,
    CopyPath,
    Undo,
    Sort(SortKey),
    Desc,
    FoldersFirst,
    Hidden,
    MakeDefault,
    Connect,
    Disconnect,
    NewWindow,
    ShowWindows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FilesHit {
    Tab(RibbonTab),
    Collapse,
    Cmd(Cmd),
    Back,
    Forward,
    Up,
    /// A breadcrumb, by index.
    Crumb(usize),
    Filter,
    Header(SortKey),
    /// A shown row, by position in the shown order.
    Row(usize),
    /// List space below the rows.
    List,
    /// A scrollbar of the list (B32).
    Bar(Axis),
}

/// What app.rs does for a **Files** window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Request {
    /// Read the shown folder (again) off the UI thread.
    List,
    Connect(String),
    Disconnect(String),
    NewWindow(String),
    ShowWindows(String),
    MachineMenu(String, (u16, u16)),
    OpenTerminal(String, PathBuf),
    /// Save this window's view of its location kind as the default.
    MakeDefault,
    /// Put these paths on the **File Clipboard**.
    Clip {
        paths: Vec<PathBuf>,
        cut: bool,
    },
    /// Paste the **File Clipboard** into this folder.
    Paste(PathBuf),
    Ask(Ask),
    Op(ops::Op),
    Undo,
    /// Put this text on the system clipboard (OSC 52).
    CopyPath(String),
}

/// A question before an operation: a dialog's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ask {
    Rename(PathBuf),
    /// Create a folder in this one.
    NewFolder(PathBuf),
    /// Permanently delete (Shift+Del).
    Delete(Vec<PathBuf>),
}

/// The **File Clipboard**: paths on one **Machine**, cut or copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Clip {
    pub machine: String,
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

/// State every **Files** window shares: the **File Clipboard** and the undo stack.
#[derive(Debug, Default)]
pub(super) struct Shared {
    pub clip: Option<Clip>,
    /// Each entry undoes one move or rename: its **Machine** and `(now, was)` pairs.
    pub undo: Vec<(String, Vec<(PathBuf, PathBuf)>)>,
}

pub(super) struct Files {
    pub win: WinId,
    pub location: Location,
    back: Vec<Location>,
    forward: Vec<Location>,
    pub views: Views,
    pub filter: String,
    /// Keys go to the filter box.
    pub filtering: bool,
    entries: Vec<Entry>,
    /// Indices into `entries` in shown order (folders).
    shown: Vec<usize>,
    pub cursor: usize,
    pub scroll: usize,
    /// First content column shown when the columns are wider than the list (B32).
    pub hscroll: usize,
    /// Vertical and horizontal bar tracks with their extents, from the last draw.
    pub bars: [Option<(Rect, Extent)>; 2],
    /// List rows (grid: cell rows) that fit, from the last draw.
    pub page: usize,
    /// Items per row: 1 in List, the cells across in Grid; from the last draw.
    pub cols: usize,
    pub tab: RibbonTab,
    pub collapsed: bool,
    pub error: Option<String>,
    /// The last operation's summary.
    pub notice: Option<String>,
    pub loading: bool,
    /// Folder modification time of the last listing; a change re-lists it.
    pub stamp: Option<SystemTime>,
    pub free: Option<u64>,
    /// Tells the current listing apart from one of an earlier location.
    pub generation: u64,
    last_click: Option<(usize, Instant)>,
    /// Selected folder entries by name (F3a).
    selected: std::collections::BTreeSet<String>,
    /// Shift+click ranges start here.
    anchor: usize,
    boxing: Option<select::Boxing>,
    /// Shown rows or cells with their index, from the last draw (box select).
    pub cells: Vec<(Rect, usize)>,
}

impl Files {
    pub fn new(win: WinId, location: Location, views: Views) -> Self {
        Self {
            win,
            location,
            back: Vec::new(),
            forward: Vec::new(),
            views,
            filter: String::new(),
            filtering: false,
            entries: Vec::new(),
            shown: Vec::new(),
            cursor: 0,
            scroll: 0,
            hscroll: 0,
            bars: [None, None],
            page: 1,
            cols: 1,
            tab: RibbonTab::Home,
            collapsed: false,
            error: None,
            notice: None,
            loading: false,
            stamp: None,
            free: None,
            generation: 0,
            last_click: None,
            selected: std::collections::BTreeSet::new(),
            anchor: 0,
            boxing: None,
            cells: Vec::new(),
        }
    }

    pub fn at_machines(&self) -> bool {
        self.location == Location::Machines
    }

    pub fn view(&self) -> &View {
        if self.at_machines() {
            &self.views.machines
        } else {
            &self.views.folders
        }
    }

    fn view_mut(&mut self) -> &mut View {
        if self.at_machines() {
            &mut self.views.machines
        } else {
            &mut self.views.folders
        }
    }

    /// The folder this window lists, if it lists one.
    pub fn folder(&self) -> Option<(&str, &Path)> {
        match &self.location {
            Location::Folder { machine, path } => Some((machine, path)),
            Location::Machines => None,
        }
    }

    /// Window title: the folder name and its **Machine**, or `Machines`.
    pub fn title(&self, label: impl Fn(&str) -> String) -> String {
        match &self.location {
            Location::Machines => "Machines".to_string(),
            Location::Folder { machine, path } => {
                format!("{} — {}", folder_name(path), label(machine))
            }
        }
    }

    /// Start a listing of the shown folder; app.rs runs it when this returns its generation.
    pub fn begin_listing(&mut self) -> Option<(u64, String, PathBuf)> {
        let (machine, path) = self.folder()?;
        let (machine, path) = (machine.to_string(), path.to_path_buf());
        self.generation += 1;
        self.loading = true;
        Some((self.generation, machine, path))
    }

    /// A listing arrived; stale ones (an earlier location) are dropped. The cursor stays on
    /// the entry it was on.
    pub fn listed(&mut self, listing: Listing) {
        if listing.generation != self.generation {
            return;
        }
        self.loading = false;
        self.stamp = listing.stamp;
        self.free = listing.free;
        match listing.entries {
            Ok(entries) => {
                let current = self.current_entry().map(|e| e.name.clone());
                self.entries = entries;
                self.error = None;
                self.reshow();
                self.prune_selection();
                self.cursor = current
                    .and_then(|name| {
                        self.shown
                            .iter()
                            .position(|&i| self.entries[i].name == name)
                    })
                    .unwrap_or(self.cursor)
                    .min(self.shown.len().saturating_sub(1));
            }
            Err(err) => {
                self.entries.clear();
                self.reshow();
                self.error = Some(err);
            }
        }
    }

    /// Recompute the shown folder entries after the entries, filter, or view changed.
    fn reshow(&mut self) {
        let view = self.views.folders;
        let needle = self.filter.to_lowercase();
        let mut shown: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                (view.hidden || !e.name.starts_with('.'))
                    && (needle.is_empty() || e.name.to_lowercase().contains(&needle))
            })
            .collect();
        shown.sort_by(|&a, &b| compare(&self.entries[a], &self.entries[b], &view));
        self.shown = shown;
        self.cursor = self.cursor.min(self.shown.len().saturating_sub(1));
    }

    /// `Machines` rows in shown order: Local first, then by the view's sort, filtered.
    pub fn machine_order(&self, rows: &[MachineRow]) -> Vec<usize> {
        let view = self.views.machines;
        let needle = self.filter.to_lowercase();
        let mut order: Vec<usize> = (0..rows.len())
            .filter(|&i| needle.is_empty() || rows[i].name.to_lowercase().contains(&needle))
            .collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&rows[a], &rows[b]);
            let local = (b.key == LOCAL_KEY).cmp(&(a.key == LOCAL_KEY));
            let by = match view.sort {
                SortKey::Kind => a.status.cmp(&b.status),
                _ => natural(&a.name, &b.name),
            };
            local.then(if view.desc { by.reverse() } else { by })
        });
        order
    }

    /// Number of shown rows.
    pub fn len(&self, rows: &[MachineRow]) -> usize {
        if self.at_machines() {
            self.machine_order(rows).len()
        } else {
            self.shown.len()
        }
    }

    /// Folder entries in shown order.
    pub fn shown_entries(&self) -> impl Iterator<Item = &Entry> {
        self.shown.iter().map(|&i| &self.entries[i])
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// What an operation acts on: the selection, else the cursor entry (folders only).
    pub fn targets(&self) -> Vec<PathBuf> {
        let Some((_, dir)) = self.folder() else {
            return Vec::new();
        };
        let mut names: Vec<&str> = self.selected_entries().map(|e| e.name.as_str()).collect();
        if names.is_empty() {
            names.extend(self.current_entry().map(|e| e.name.as_str()));
        }
        names.into_iter().map(|n| dir.join(n)).collect()
    }

    /// An operation finished: its summary or error goes to the status bar.
    pub fn done(&mut self, done: &ops::Done) {
        self.notice = Some(done.summary.clone());
        self.error.clone_from(&done.error);
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.shown
            .get(self.cursor)
            .and_then(|&i| self.entries.get(i))
    }

    /// The **Machine** under the cursor (`Machines`) or the one shown.
    pub fn machine<'a>(&self, rows: &'a [MachineRow]) -> Option<&'a MachineRow> {
        match &self.location {
            Location::Machines => self
                .machine_order(rows)
                .get(self.cursor)
                .and_then(|&i| rows.get(i)),
            Location::Folder { machine, .. } => rows.iter().find(|r| &r.key == machine),
        }
    }

    pub fn go(&mut self, location: Location) -> Option<Request> {
        if location == self.location {
            return None;
        }
        let old = std::mem::replace(&mut self.location, location);
        self.back.push(old);
        self.forward.clear();
        Some(self.arrive())
    }

    fn arrive(&mut self) -> Request {
        self.entries.clear();
        self.shown.clear();
        self.filter.clear();
        self.filtering = false;
        self.cursor = 0;
        self.scroll = 0;
        self.selected.clear();
        self.boxing = None;
        self.error = None;
        self.notice = None;
        self.stamp = None;
        self.free = None;
        self.loading = false;
        self.generation += 1;
        if self.at_machines() && self.tab == RibbonTab::Home {
            self.tab = RibbonTab::Machine;
        }
        Request::List
    }

    pub fn back(&mut self) -> Option<Request> {
        let to = self.back.pop()?;
        let old = std::mem::replace(&mut self.location, to);
        self.forward.push(old);
        Some(self.arrive())
    }

    pub fn forward(&mut self) -> Option<Request> {
        let to = self.forward.pop()?;
        let old = std::mem::replace(&mut self.location, to);
        self.back.push(old);
        Some(self.arrive())
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// The parent folder; `Machines` above a filesystem root.
    pub fn up(&mut self) -> Option<Request> {
        let parent = match &self.location {
            Location::Machines => return None,
            Location::Folder { machine, path } => match path.parent() {
                Some(parent) => Location::Folder {
                    machine: machine.clone(),
                    path: parent.to_path_buf(),
                },
                None => Location::Machines,
            },
        };
        self.go(parent)
    }

    /// Enter or double-click on the cursor row.
    pub fn open(&mut self, rows: &[MachineRow]) -> Option<Request> {
        match &self.location {
            Location::Machines => {
                let row = self.machine(rows)?;
                let key = row.key.clone();
                self.go(Location::home(&key))
            }
            Location::Folder { machine, path } => {
                let entry = self.current_entry()?;
                if entry.dir {
                    let to = Location::Folder {
                        machine: machine.clone(),
                        path: path.join(&entry.name),
                    };
                    self.go(to)
                } else {
                    self.error = Some("opening files comes with File Associations".into());
                    None
                }
            }
        }
    }

    fn set_view(&mut self, change: impl FnOnce(&mut View)) {
        change(self.view_mut());
        if !self.at_machines() {
            self.reshow();
        }
    }

    /// A header click sorts by the column; clicking the sorted column reverses it.
    fn sort_by(&mut self, key: SortKey) {
        self.set_view(|v| {
            if v.sort == key {
                v.desc = !v.desc;
            } else {
                v.sort = key;
                v.desc = false;
            }
        });
    }

    pub fn move_cursor(&mut self, delta: isize, rows: &[MachineRow]) {
        let last = self.len(rows).saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.select_cursor();
        self.keep_visible();
    }

    /// Shift+wheel or wheel left/right: move the columns.
    pub fn hscroll_by(&mut self, delta: isize) {
        let max = self.bars[1].map_or(0, |(_, e)| e.max());
        self.hscroll = self.hscroll.saturating_add_signed(delta).min(max);
    }

    /// Lines of the list: items in List, rows of cells in Grid.
    pub fn lines(&self, rows: &[MachineRow]) -> usize {
        self.len(rows).div_ceil(self.cols.max(1))
    }

    /// Wheel: move the list, not the cursor.
    pub fn scroll_by(&mut self, delta: isize, rows: &[MachineRow]) {
        let max = self.lines(rows).saturating_sub(self.page);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    pub fn keep_visible(&mut self) {
        let page = self.page.max(1);
        let line = self.cursor / self.cols.max(1);
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + page {
            self.scroll = line + 1 - page;
        }
    }

    pub fn click(
        &mut self,
        hit: FilesHit,
        right: bool,
        mods: crossterm::event::KeyModifiers,
        at: (u16, u16),
        rows: &[MachineRow],
    ) -> Option<Request> {
        if !matches!(hit, FilesHit::Filter) {
            self.filtering = false;
        }
        match hit {
            FilesHit::Tab(tab) => {
                self.tab = tab;
                None
            }
            FilesHit::Collapse => {
                self.collapsed = !self.collapsed;
                None
            }
            FilesHit::Cmd(cmd) => self.command(cmd, rows),
            FilesHit::Back => self.back(),
            FilesHit::Forward => self.forward(),
            FilesHit::Up => self.up(),
            FilesHit::Crumb(index) => {
                let to = crumbs(&self.location, |m| m.to_string())
                    .into_iter()
                    .nth(index)?
                    .1;
                self.go(to)
            }
            FilesHit::Filter => {
                self.filtering = true;
                None
            }
            FilesHit::Header(key) => {
                self.sort_by(key);
                None
            }
            FilesHit::Row(index) => {
                if !(right && self.is_selected(index)) {
                    self.select_click(index, mods);
                }
                let now = Instant::now();
                let double = matches!(self.last_click,
                    Some((last, at)) if last == index && now.duration_since(at) < DOUBLE_CLICK);
                self.last_click = Some((index, now));
                if right {
                    let key = self.at_machines().then(|| self.machine(rows)).flatten()?;
                    return Some(Request::MachineMenu(key.key.clone(), at));
                }
                if double {
                    self.last_click = None;
                    return self.open(rows);
                }
                None
            }
            FilesHit::List | FilesHit::Bar(_) => None,
        }
    }

    pub fn command(&mut self, cmd: Cmd, rows: &[MachineRow]) -> Option<Request> {
        let machine = self.machine(rows).map(|r| r.key.clone());
        match cmd {
            Cmd::Open => self.open(rows),
            Cmd::OpenTerminal => {
                let (machine, path) = self.folder()?;
                let mut path = path.to_path_buf();
                if let Some(entry) = self.current_entry().filter(|e| e.dir) {
                    path.push(&entry.name);
                }
                Some(Request::OpenTerminal(machine.to_string(), path))
            }
            Cmd::Refresh => Some(Request::List),
            Cmd::Cut | Cmd::Copy => {
                let paths = self.targets();
                (!paths.is_empty()).then_some(Request::Clip {
                    paths,
                    cut: cmd == Cmd::Cut,
                })
            }
            Cmd::Paste => self
                .folder()
                .map(|(_, dir)| Request::Paste(dir.to_path_buf())),
            Cmd::Rename => {
                let path = self.targets().into_iter().next()?;
                Some(Request::Ask(Ask::Rename(path)))
            }
            Cmd::NewFolder => {
                let dir = self.folder()?.1.to_path_buf();
                Some(Request::Ask(Ask::NewFolder(dir)))
            }
            Cmd::Trash => {
                let paths = self.targets();
                (!paths.is_empty()).then_some(Request::Op(ops::Op::Trash(paths)))
            }
            Cmd::Delete => {
                let paths = self.targets();
                (!paths.is_empty()).then_some(Request::Ask(Ask::Delete(paths)))
            }
            Cmd::CopyPath => {
                let paths = self.targets();
                let text = paths
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.is_empty()).then_some(Request::CopyPath(text))
            }
            Cmd::Undo => Some(Request::Undo),
            Cmd::Layout(layout) => {
                self.set_view(|v| v.layout = layout);
                None
            }
            Cmd::Sort(key) => {
                self.set_view(|v| v.sort = key);
                None
            }
            Cmd::Desc => {
                self.set_view(|v| v.desc = !v.desc);
                None
            }
            Cmd::FoldersFirst => {
                self.set_view(|v| v.folders_first = !v.folders_first);
                None
            }
            Cmd::Hidden => {
                self.set_view(|v| v.hidden = !v.hidden);
                None
            }
            Cmd::MakeDefault => Some(Request::MakeDefault),
            Cmd::Connect => machine.map(Request::Connect),
            Cmd::Disconnect => machine.map(Request::Disconnect),
            Cmd::NewWindow => machine.map(Request::NewWindow),
            Cmd::ShowWindows => machine.map(Request::ShowWindows),
        }
    }

    /// Keys while the window has focus.
    pub fn key(&mut self, chunk: &[u8], rows: &[MachineRow]) -> Option<Request> {
        if self.filtering {
            match chunk {
                b"\x1b" => {
                    self.filter.clear();
                    self.filtering = false;
                }
                b"\r" | b"\n" => self.filtering = false,
                b"\x7f" | b"\x08" => {
                    self.filter.pop();
                }
                _ => match std::str::from_utf8(chunk) {
                    Ok(text) if !text.chars().any(char::is_control) => self.filter.push_str(text),
                    _ => return None,
                },
            }
            self.cursor = 0;
            self.scroll = 0;
            if !self.at_machines() {
                self.reshow();
                self.prune_selection();
            }
            return None;
        }
        let cols = isize::try_from(self.cols.max(1)).unwrap_or(1);
        let page = isize::try_from(self.page.max(1)).unwrap_or(1) * cols;
        let grid = self.view().layout == Layout::Grid;
        match chunk {
            b"\x1b[A" | b"\x1bOA" => self.move_cursor(-cols, rows),
            b"\x1b[B" | b"\x1bOB" => self.move_cursor(cols, rows),
            b"\x1b[D" | b"\x1bOD" if grid => self.move_cursor(-1, rows),
            b"\x1b[C" | b"\x1bOC" if grid => self.move_cursor(1, rows),
            b"\x1b[5~" => self.move_cursor(-page, rows),
            b"\x1b[6~" => self.move_cursor(page, rows),
            b"\x1b[H" | b"\x1bOH" | b"\x1b[1~" => self.move_cursor(isize::MIN / 2, rows),
            b"\x1b[F" | b"\x1bOF" | b"\x1b[4~" => self.move_cursor(isize::MAX / 2, rows),
            b"\r" | b"\n" => return self.open(rows),
            b"\x7f" | b"\x08" => return self.up(),
            b"\x1b[1;3D" | b"\x1bb" => return self.back(),
            b"\x1b[1;3C" | b"\x1bf" => return self.forward(),
            b"/" | b"\x06" => self.filtering = true,
            b"\x12" => return Some(Request::List),
            b"\x18" => return self.command(Cmd::Cut, rows),
            b"\x03" => return self.command(Cmd::Copy, rows),
            b"\x16" => return self.command(Cmd::Paste, rows),
            b"\x1a" => return self.command(Cmd::Undo, rows),
            b"\x1bOQ" | b"\x1b[12~" => return self.command(Cmd::Rename, rows),
            b"\x1b[3~" => return self.command(Cmd::Trash, rows),
            b"\x1b[3;2~" => return self.command(Cmd::Delete, rows),
            b" " => self.toggle_cursor(),
            b"\x01" => self.select_all(),
            b"\x1b" => self.clear_selection(),
            _ => {}
        }
        None
    }
}

/// Folders first (when set), then the sort key, then the name.
fn compare(a: &Entry, b: &Entry, view: &View) -> std::cmp::Ordering {
    let folders = if view.folders_first {
        b.dir.cmp(&a.dir)
    } else {
        std::cmp::Ordering::Equal
    };
    let by = match view.sort {
        SortKey::Name => natural(&a.name, &b.name),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Kind => a.kind.cmp(&b.kind),
    }
    .then_with(|| natural(&a.name, &b.name));
    folders.then(if view.desc { by.reverse() } else { by })
}

/// Case-insensitive with digit runs compared as numbers (`f9` before `f10`), then
/// case-sensitive so the order is total.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (la, lb) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (la.chars().peekable(), lb.chars().peekable());
    loop {
        let (Some(&cx), Some(&cy)) = (x.peek(), y.peek()) else {
            return x
                .peek()
                .is_some()
                .cmp(&y.peek().is_some())
                .then_with(|| a.cmp(b));
        };
        if cx.is_ascii_digit() && cy.is_ascii_digit() {
            let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                let mut digits = String::new();
                while let Some(c) = it.next_if(char::is_ascii_digit) {
                    digits.push(c);
                }
                digits.trim_start_matches('0').to_string()
            };
            let (nx, ny) = (run(&mut x), run(&mut y));
            let by = nx.len().cmp(&ny.len()).then_with(|| nx.cmp(&ny));
            if by.is_ne() {
                return by;
            }
        } else {
            if cx != cy {
                return cx.cmp(&cy);
            }
            x.next();
            y.next();
        }
    }
}

pub(super) fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// The home folder of `machine`: this user's, or `~` on a remote **Machine**.
pub(super) fn home_of(machine: &str) -> PathBuf {
    if machine == LOCAL_KEY {
        home()
    } else {
        remote::home()
    }
}

fn folder_name(path: &Path) -> String {
    if path == home() || path == remote::home() {
        return "~".to_string();
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// `Machines › Local › ~ › repos`: each crumb with the location it opens.
pub(super) fn crumbs(
    location: &Location,
    label: impl Fn(&str) -> String,
) -> Vec<(String, Location)> {
    let mut out = vec![("Machines".to_string(), Location::Machines)];
    let Location::Folder { machine, path } = location else {
        return out;
    };
    let at = |path: PathBuf| Location::Folder {
        machine: machine.clone(),
        path,
    };
    out.push((label(machine), Location::home(machine)));
    let home = home_of(machine);
    let (mut base, rest) = match path.strip_prefix(&home) {
        Ok(rest) => {
            out.push(("~".to_string(), at(home.clone())));
            (home.clone(), rest.to_path_buf())
        }
        Err(_) => {
            let root: PathBuf = path
                .ancestors()
                .last()
                .map_or_else(|| "/".into(), Path::to_path_buf);
            out.push((root.display().to_string(), at(root.clone())));
            let rest = path
                .strip_prefix(&root)
                .map_or_else(|_| PathBuf::new(), Path::to_path_buf);
            (root, rest)
        }
    };
    for part in rest.components() {
        base.push(part);
        out.push((
            part.as_os_str().to_string_lossy().into_owned(),
            at(base.clone()),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn entry(name: &str, dir: bool, size: u64) -> Entry {
        Entry {
            name: name.into(),
            dir,
            size,
            modified: None,
            kind: if dir { "folder".into() } else { "file".into() },
        }
    }

    pub(super) fn files_with(entries: Vec<Entry>) -> Files {
        let mut f = Files::new(1, Location::home(LOCAL_KEY), Views::default());
        let generation = f.begin_listing().map(|(g, ..)| g).unwrap_or(0);
        f.listed(Listing {
            generation,
            entries: Ok(entries),
            stamp: None,
            free: None,
        });
        f
    }

    #[test]
    fn grid_layout_loads_by_default_as_list_and_moves_by_rows() {
        let old: View = serde_json::from_str(r#"{"sort":"size"}"#).unwrap_or_default();
        assert_eq!((old.layout, old.sort), (Layout::List, SortKey::Size));
        let mut f = files_with((0..10).map(|i| entry(&format!("f{i}"), false, 1)).collect());
        f.command(Cmd::Layout(Layout::Grid), &[]);
        assert_eq!(f.view().layout, Layout::Grid);
        let saved = serde_json::to_string(f.view()).unwrap_or_default();
        assert!(saved.contains(r#""layout":"grid""#), "{saved}");
        (f.cols, f.page) = (3, 2);
        f.key(b"\x1b[B", &[]);
        f.key(b"\x1b[C", &[]);
        assert_eq!(f.cursor, 4);
        f.key(b"\x1b[B", &[]);
        f.key(b"\x1b[B", &[]);
        // Item 10 is on line 3 of 4: the view scrolls one line.
        assert_eq!((f.cursor, f.scroll), (9, 2));
        assert_eq!(f.lines(&[]), 4);
    }

    fn names(f: &Files) -> Vec<&str> {
        f.shown_entries().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn folders_come_first_and_sorting_toggles() {
        let mut f = files_with(vec![
            entry("b.txt", false, 30),
            entry("Docs", true, 0),
            entry("a.txt", false, 10),
            entry(".hidden", false, 1),
        ]);
        assert_eq!(names(&f), ["Docs", "a.txt", "b.txt"]);
        f.click(
            FilesHit::Header(SortKey::Size),
            false,
            crossterm::event::KeyModifiers::NONE,
            (0, 0),
            &[],
        );
        assert_eq!(names(&f), ["Docs", "a.txt", "b.txt"]);
        f.click(
            FilesHit::Header(SortKey::Size),
            false,
            crossterm::event::KeyModifiers::NONE,
            (0, 0),
            &[],
        );
        assert!(f.view().desc);
        assert_eq!(names(&f), ["Docs", "b.txt", "a.txt"]);
        f.command(Cmd::Hidden, &[]);
        f.command(Cmd::FoldersFirst, &[]);
        assert_eq!(names(&f), ["b.txt", "a.txt", ".hidden", "Docs"]);
    }

    #[test]
    fn names_sort_with_numbers_as_numbers() {
        let mut names = ["f10", "F2", "f1", "f02", "g"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["f1", "F2", "f02", "f10", "g"]);
    }

    #[test]
    fn filter_narrows_and_escape_clears() {
        let mut f = files_with(vec![entry("alpha", false, 0), entry("beta", false, 0)]);
        f.key(b"/", &[]);
        f.key(b"ph", &[]);
        assert_eq!(names(&f), ["alpha"]);
        f.key(b"\x1b", &[]);
        assert_eq!(names(&f), ["alpha", "beta"]);
        assert!(!f.filtering);
    }

    #[test]
    fn history_goes_back_and_forward_and_drops_stale_listings() {
        let mut f = files_with(vec![entry("repos", true, 0)]);
        let stale = f.generation;
        assert_eq!(f.key(b"\r", &[]), Some(Request::List));
        assert_eq!(
            f.location,
            Location::Folder {
                machine: LOCAL_KEY.into(),
                path: home().join("repos")
            }
        );
        f.listed(Listing {
            generation: stale,
            entries: Ok(vec![entry("x", false, 0)]),
            stamp: None,
            free: None,
        });
        assert_eq!(f.entry_count(), 0);
        f.back();
        assert_eq!(f.location, Location::home(LOCAL_KEY));
        f.forward();
        assert!(f.can_back() && !f.can_forward());
        f.up();
        assert_eq!(f.location, Location::home(LOCAL_KEY));
    }

    #[test]
    fn crumbs_name_home_and_each_folder() {
        let at = Location::Folder {
            machine: LOCAL_KEY.into(),
            path: home().join("repos/sshdr"),
        };
        let labels: Vec<String> = crumbs(&at, |_| "Local".into())
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(labels, ["Machines", "Local", "~", "repos", "sshdr"]);
        let root = Location::Folder {
            machine: LOCAL_KEY.into(),
            path: "/".into(),
        };
        let mut f = Files::new(1, root, Views::default());
        f.up();
        assert!(f.at_machines());
    }

    #[test]
    fn machines_keep_local_first_and_open_local_home() {
        let row = |key: &str, name: &str| MachineRow {
            key: key.into(),
            name: name.into(),
            target: String::new(),
            status: String::new(),
            alert: false,
            on: false,
        };
        let rows = [
            row("zeta", "zeta"),
            row(LOCAL_KEY, "Local"),
            row("alpha", "alpha"),
        ];
        let mut f = Files::new(1, Location::Machines, Views::default());
        assert_eq!(f.machine_order(&rows), [1, 2, 0]);
        f.views.machines.desc = true;
        assert_eq!(f.machine_order(&rows), [1, 0, 2]);
        f.move_cursor(1, &rows);
        assert_eq!(f.open(&rows), Some(Request::List));
        assert_eq!(f.location, Location::home("zeta"));
        f.location = Location::Machines;
        f.cursor = 0;
        assert_eq!(f.open(&rows), Some(Request::List));
        assert_eq!(f.location, Location::home(LOCAL_KEY));
    }
}
