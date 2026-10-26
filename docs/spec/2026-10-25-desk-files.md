# Desk Files

Status: Approved (2026-10-25); F1 and F2 landed 2026-10-26
Backlog: B16 (Local), B17 (remote); B27 decided by [ADR 0009](../adr/0009-files-is-one-app-task.md)

One file manager **App** for every **Machine**, drawn by the **Desktop** as **Native Windows**.
Local comes first; remote **Machines** follow (B17).

## Window

```
┌ repos — Local ───────────────────────────────────[_][□][×]┐
│ Home  View  Machine                                     «  │  ribbon tabs (« collapses)
│ ✂Cut ⧉Copy ⎘Paste │ Rename Trash │ +Folder │ ▸Terminal   │  commands of the shown tab
│ ‹ › ↑  Machines › Local › ~ › repos            ⌕ filter   │  address bar
│ Name ▲               Size     Modified      Kind           │  list header
│ ▸ docs/                       10-25 14:02   folder         │
│ 12 items · 3 selected (4.2 MB) · 210 GB free · list · name▲│  status bar
└────────────────────────────────────────────────────────────┘
```

### Locations

- `Machines`: Local and every saved **Machine** (the former **Machines Folder**), with Target and
  Status (`Server · n`, `SSH · n`, not connected).
- A folder on a **Machine**: `Machines › <machine> › <path>`; breadcrumbs are clickable.
- Back, Forward, Up; Backspace goes up; a filter box narrows the shown entries.
- Double click on a **Machine** browses its files (Local now; remote after B17, until then a
  remote **Machine** connects as today). Connect, New window, Disconnect stay on the Machine
  ribbon tab and the right-click menu.

### Ribbon

Two rows: tabs, then the shown tab's commands. `«` collapses it to the tab row, as the side
**Taskbar** collapses (B30).

| Tab | Commands |
|---|---|
| Home | Cut, Copy, Paste, Rename, Trash, New folder, Open, Open in terminal |
| View | List or Grid, Sort by, Ascending or Descending, Hidden files, Folders first, Apply to all folders |
| Machine (contextual: a **Machine** selected or shown) | Connect or Disconnect, New window, Show windows |

### Views

- **List**: columns. Folders: Name, Size, Modified, Kind. `Machines`: Name, Target, Status.
  Clicking a header sorts by it; clicking again reverses.
- **Grid**: cells about 14×3: a glyph and the name (two lines, cut). Glyphs are plain Unicode,
  not Nerd Font; a **Machine** cell adds its status.

### Sort

Name, Size, Modified, Kind (`Machines`: Name, Status); ascending or descending; folders first by
default.

### View and sort state

**Client** presentation state, saved in `desk.json` only (ADR 0009):

1. A default per location kind (`Machines`, folders); View › Apply to all folders sets it.
2. Each open window's own view and sort, saved and restored with its geometry.

### Status bar

Item count, selection count and size, free space of the volume, view and sort, the **Machine**'s
connection state, background operation progress, and the last error.

## With Terminal Windows

1. **Open in terminal** opens a tab with the folder as its working directory in the most recently
   used **Terminal Window** of the same **Machine**, or a new window when it has none.
2. **Open in Files** (a **Terminal Window**'s title or tab right-click) opens the pane's
   `foreground_cwd` (or `cwd`).
3. Dropping files on a pane sends their shell-quoted paths (`pane.send_text`).
4. Opening a file runs its **File Association** in a **Terminal Window** of the file's
   **Machine** (with B18).
5. Later: a **Files** window that follows a pane's working directory.

## Selection, clipboard, operations

- Click, Shift+click range, Ctrl+click toggle (Space as a fallback where the terminal keeps
  Ctrl+click), drag a box on empty space, `^A`.
- Drag and drop between **Files** windows and onto folders: move on the same **Machine**, copy
  across **Machines**.
- **File Clipboard**: **Machine**, paths, cut or copy; Copy path also puts text on the system
  clipboard (OSC 52).
- Trash moves to the platform trash (`~/.Trash`, freedesktop trash); permanent delete only with
  Shift+Del and a confirmation. Rename and move can be undone.
- Listing and file operations run off the UI thread; an open folder refreshes when its
  modification time changes on the idle tick (no new dependency).

## Code

`src/cli/desk/files/`: location and sort model, list and grid drawing, ribbon, background
operations. `app.rs` only routes events to it.

## Phases

| Phase | Scope |
|---|---|
| F1 | Window, locations, list, sort, status bar, ribbon, saved view and sort; `Machines` replaces the **Machines Folder** |
| F2 | Grid |
| F3 | Selection, **File Clipboard**, rename, new folder, trash, drag and drop (onto panes too) |
| F4 | **File Associations** (with B18) |
| F5 | Remote **Machines** (B17) |
