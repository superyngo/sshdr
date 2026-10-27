# Desktop client (`herdr desk`)

The **Desktop** is a separate **Client** (ADR 0006) that shows the local **Server**'s Session, and
the Sessions of connected **Server Tier** **Machines**, as overlapping **Terminal Windows**. Mapping
(ADR 0008): every **Machine** (and Local) is a **Task**, every workspace a **Terminal Window** with
a tab strip, showing the shown tab's panes as the **Server** lays them out. Code: `src/cli/desk/`.

## Running

```bash
herdr desk                      # default Session; needs a running server
HERDR_SESSION=<name> herdr desk # another Session
```

Quitting the desk leaves every workspace, tab, and pane running on the **Server**.

## Screen

| Area | Mouse | Effect |
|---|---|---|
| Title bar | drag | move (edges snap to a hidden 12×6 grid); releasing at the left or right desk edge fills that half, at the top edge maximizes |
| Title bar | double click | maximize / restore |
| Menu taller than the desk | wheel; scrollbar on its right border: click, drag | scroll it (arrow keys keep the highlight shown) |
| Title bar | right click | menu: Rename… (`workspace rename`), New tab, Minimize, Maximize, Close |
| `[_]` `[□]` `[×]` | click | minimize, maximize / restore, close the workspace (`workspace close`; asks first when it has more than one tab) |
| Tab strip: tab | click | show that tab (`tab focus`, so every **Client** shows it) |
| Tab strip: tab | right click | Rename tab… (`tab rename`), Close tab |
| Tab strip: `×` after a tab | click | close the tab (`tab close`); the last tab closes the window |
| Tab strip: `+` | click | new tab (`tab create`), shown at once |
| Pane | click | focus it (`pane.focus`) and pass the click on |
| Divider between panes | drag | resize the split (`layout.set_split_ratio`, ratio 0.1–0.9) |
| Border | drag | resize |
| Content | any | forwarded to the pane under the pointer |
| `⊞ Start` | click | **Start Menu**: Files (Local home), Machines (**Files** at the `Machines` root; raises one already there), Tile all, Quit desk |
| **Task** button | click | the **Machine**'s windows (`▁` = minimized); choosing one restores and raises it |
| **Task** button | right click | New window, Minimize all, Restore all, then Disconnect (connected **Machine**, **Server** or **SSH Tier**) or Connect |
| **Taskbar** space without a button | drag | move the bar to the screen edge nearest the pointer (the landing place is highlighted) |
| **Taskbar** space without a button | right click | Top, Bottom, Left, Right (`•` marks the current one), Collapse or Expand (left or right only), Larger, Smaller (not while collapsed) |
| Thin line along the **Taskbar** (`▁▔▏▕`, under windows) | drag | resize: 1–4 rows at the top or bottom, 12–40 columns at the left or right |

| **Files**: `Machines` row | double click, Enter | browse its home folder; a remote **Machine** is read with `sh` over ssh (both Tiers, no remote `herdr`), re-listed on Refresh and after operations rather than polled; Windows **Machines** report "not supported yet". Connect is on the Machine ribbon tab and the right-click menu |
| **Files**: `Machines` row | right click | Connect (not connected); Show windows, New window, Disconnect (connected, **Server** or **SSH Tier** alike); Show windows, New window (Local) |
| `«` / `»` on the last row of a left or right **Taskbar** | click | collapse it to 4 columns / expand it again (as herdr's sidebar) |

A top or bottom **Taskbar** wraps its **Task** buttons onto further rows; a left or right one
shows Start on the first row, one **Task** per row below, and the hint and `«` on the last row.

## Files

**Files** is the file manager **App** (spec `docs/spec/2026-10-25-desk-files.md`, ADR 0009); all
its windows share one **Files** **Task** after the **Machine** buttons. A window shows the
`Machines` root (the former **Machines Folder**) or a folder on Local; F1 builds the list view.

| Part | Use |
|---|---|
| Ribbon tabs Home, View, Machine; `«`/`»` | choose the command row; collapse the ribbon to the tab row (saved per window) |
| Home | Open, Open in terminal (a new tab working in the folder, or the folder under the cursor, in the most recently used Local **Terminal Window**; a new window when there is none), Refresh; Cut (`^X`), Copy (`^C`), Paste (`^V`; a name taken gets ` (2)`, a cut pastes once), Rename (F2), Trash (Del; macOS `~/.Trash`, freedesktop trash on Linux, same volume only, not on Windows yet), New folder, Copy path (system clipboard over OSC 52), Undo (`^Z`: moves and renames); Shift+Del deletes permanently after a confirmation. They act on the selection, else the item under the cursor; Local only; the result shows in the status bar |
| View | List or Grid (cells of a glyph and a two-line name; ←/→ move by one, ↑/↓ by a row of cells), sort by name, size, modified, kind (`Machines`: name, status), ascending or descending, folders first, hidden files (`.` names), Apply to all folders (`Machines`: Make default) |
| Machine (the **Machine** under the cursor, or the one shown) | Connect or Disconnect, New window, Show windows |
| `‹ › ↑` and breadcrumbs `Machines › Local › ~ › repos` | back, forward, parent (`Machines` above `/`); a crumb opens that place; long paths drop middle crumbs as `…` |
| `⌕ filter` (click, `/`, `^F`) | narrows the shown names as typed; Enter keeps it, Esc clears it |
| Column header | sort by it; again reverses (`▲`/`▼`) |
| Row | click selects; double click or Enter opens a folder (files open with **File Associations**, F4) |
| Drag selected items (folders) | onto a folder row or cell, or another **Files** window: move there (Ctrl held at the drop: copy); onto a pane: type their shell-quoted paths, no Enter (`pane send-text`); a ` n items ` label follows the pointer |
| Selection (folders) | Shift+click a range, Ctrl+click or Space toggles one, `^A` all, Esc none, drag a box from empty list space (Ctrl adds); arrow keys select the item they move to; the status bar shows the count and size |
| Wheel; Shift+wheel or wheel left/right | scrolls the list up and down; sideways when the columns are wider than the window (Name keeps at least 24 columns) |
| Scrollbars (right edge, bottom row; shown on overflow) | click jumps there, drag the thumb |
| Keys | arrows, PgUp/PgDn, Home/End move; Backspace parent; Alt+←/→ back/forward; `^R` refresh |
| Status bar | items (shown of total), selection and its size, free space on the volume, view and sort, connection state, loading, last error |

Folders are read on a background thread. Every second the desk checks each open Local folder's
modification time and lists it again when it changed. Names sort case-insensitively with numbers
compared as numbers (`f9` before `f10`).
Collapsed, it shows `⊞`, each **Task** as its first character and window count (`L4`), and `»`;
its buttons work as before, and the bar cannot be resized until expanded. Menus
opened from the bar open on the desk side of it; maximized windows follow the bar.

Menus: arrow keys move, Enter chooses, Esc closes. Dialogs: Enter confirms, Esc cancels.

## Keys

Ctrl+\ then: `n` new window (workspace) in the focused window's **Task** · `w` close · `z` minimize · `m` maximize · `t` tile · `b` collapse or expand a side **Taskbar** · `Tab` next ·
`q` quit · Ctrl+\ sends a literal Ctrl+\. Everything else goes to the focused pane of the focused window.

## Server state

- State is read over the public API socket: `session.snapshot` for workspaces, tabs, panes, and
  layouts, and one `events.subscribe` stream per **Server** (workspace created, renamed, closed;
  tab created, closed, renamed, moved, focused; pane created, closed, focused; layout updated). Each burst of events (100 ms) brings one new
  snapshot, so changes made by any other **Client** appear on the desk. An idle desk starts no
  `herdr` processes; only the ping of a connected **Machine** (below) runs `ssh -W` every 3 s.
- Changes use public CLI commands: `workspace create|rename|close`, `tab create|rename|close|focus`,
  `pane run`; focusing a pane uses the `pane.focus` API request (the CLI focuses only by
  direction). Each shown pane is one `terminal session control` child.
- A window shows the workspace's `active_tab_id`. The tab's panes come from its `layouts` entry:
  their rects are scaled from the **Server**'s layout area into the window, with one cell between
  neighbours for a divider; a zoomed tab shows only its focused pane. Keys go to the layout's
  `focused_pane_id`. Dragging a divider sends `layout.set_split_ratio` for the split under it
  (its path from the `splits` id) every 33 ms and on release, off the UI thread with one call in
  flight; the `layout_updated` event redraws the panes. Without a layout entry the tab shows its root pane (`TabInfo.root_pane_id`,
  an optional field added by sshdr; against a server without it, the first pane without a label).
  Dividers cannot be dragged.

## Machines

The `Machines` root of **Files** lists Local, every enabled saved **Machine** (`herdr machine add`, read
from the endpoint catalog), and the **Machines** saved with `herdr machine add --ssh-only`
(`ssh-machines.json`, default Session); both files are read only. The **Tier** of every
**Machine** is detected on connect, whichever file it came from. Status per row: `Local`, `not connected`,
`connecting…`, `Server` (with its window count), `SSH`, `! auth`, `! offline`.

Connecting runs `herdr status server --json` on the **Machine** over SSH, which reports its API
socket, and detects the **Tier** each time (ADR 0004):

| Result | Status | Effect |
|---|---|---|
| Server running | `Server` | the **Machine** gets a **Taskbar** button; its workspaces open as windows titled `<machine>: <workspace>`; followed by events like Local |
| ssh exit 255 with an authentication or host-key error | `! auth` | nothing opens; no prompt (`BatchMode=yes`) |
| other ssh exit 255, or no answer in 12 s | `! offline` | nothing opens |
| ssh works, `herdr` is missing, fails, or its server is not running | `SSH` | a new local workspace named after the **Machine** runs `ssh <target>`; its window, and every tab added to it (each runs `ssh` too), belong to the **Machine**'s **Task** |

Remote commands run as `env HERDR_SESSION=<profile session> herdr …`, and API requests and the
event stream as `ssh -W <socket>`, over the managed ControlMaster (`platform::shared_ssh_control_path`,
the same socket herdr itself uses); no newer herdr is needed on the **Machine**. Each master
carries at most 8 pane streams; more windows open another master, started before its streams.

A **Machine** without events for 3 s is pinged. A failed ping (3 s limit), snapshot (5 s), or a
closed event stream marks it `! offline` (shown in the **Taskbar** hint, within about 7 s of a
stall) and prefixes its window titles with `⚠`; windows keep their place, and the desk retries
every 2 s. A window whose stream ends stays put and asks for a snapshot: it reattaches in place,
or closes when its workspace is gone. Disconnect stops following the **Machine** and closes its
windows; its workspaces keep running.

The same menu items act on both **Tiers**; only the behavior and the status (`Server · n`,
`SSH · n`) differ:

| Item | **Server Tier** | **SSH Tier** |
|---|---|---|
| Connect | open its workspaces as windows | open the first `ssh` window; the **Machine** then counts as connected |
| New window | `workspace create` on the **Machine** | another local `ssh` window in its **Task** |
| Disconnect | close its windows; its workspaces keep running | close its `ssh` windows (their workspaces end), after asking when any are open |

Both are saved as connected in `desk.json` and reconnect on start; an **SSH Tier** **Machine**
reconnects without opening a window.

## Saved layout

The desk writes `desk.json` in the Session data directory (`crate::session::data_dir()`) once the
layout has been unchanged for 1 s, and on quit (temporary file, then rename):
per window the position, size, maximize state, minimize state, and stacking order, keyed by the
workspace id (local) or `<profile id>/<workspace id>` (**Machine**) in `workspace`, plus the
**Machines** that were connected, the **Taskbar** edge, size, and collapse state (`taskbar`, omitted at the
default bottom, 1 row), the local `ssh` workspaces with their **Machine** (`ssh_workspaces`), the
**Files** view and sort defaults per location kind (`files_views`: `machines`, `folders`), and each
**Files** window (`files`: geometry, location, its own views, ribbon collapse). A new **Files**
window starts with the defaults; View › Apply to all folders (or Make default) sets them. Nothing
is remembered per folder.
Entries of builds before B25 (keyed by terminal id) match no window, so those windows open
stacked once.
The next start restores the layout and reconnects those **Machines**; a **Machine**'s windows get their saved place when they open. Windows without an entry open stacked on top.

## Limits

- Remote **Machines** need OpenSSH ControlMaster (Unix); **Files** cannot add or
  remove **Machines** (`herdr machine add|remove`).
- Kitty keyboard protocol is not negotiated over `terminal session control` (legacy key bytes only).
- Choosing a tab or pane also changes the **Server**'s focused workspace, tab, and pane, so a herdr
  shell **Client** on the same **Session** follows the desk, and the other way round (ADR 0008).
- **Files** browses remote **Machines** but changes files on Local only (B17 F5b, F5c); Windows
  **Machines** cannot be browsed.
- Not yet run on Windows (V2).
