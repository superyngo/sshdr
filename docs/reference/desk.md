# Desktop client (`herdr desk`)

The **Desktop** is a separate **Client** (ADR 0006) that shows the local **Server**'s Session, and
the Sessions of connected **Server Tier** **Machines**, as overlapping **Terminal Windows**. Mapping (ADR 0007): every workspace is a **Task**, every tab a
**Terminal Window** showing the tab's root pane. Code: `src/cli/desk/`.

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
| Title bar | right click | menu: Rename…, Minimize, Maximize, Close |
| `[_]` `[□]` `[×]` | click | minimize, maximize / restore, close (`tab close`) |
| Border | drag | resize |
| Content | any | forwarded to the pane |
| `▣ Machines` icon | double click | open the **Machines Folder** |
| `⊞ Start` | click | **Start Menu**: Machines, New window, New task, Tile all, Quit desk |
| **Task** button | click | the Task's windows (`▁` = minimized); choosing one restores and raises it |
| **Task** button | right click | Rename…, New window, Minimize all, Restore all, Close task… (confirms) |
| **Taskbar** space without a button | drag | move the bar to the screen edge nearest the pointer (the landing place is highlighted) |
| **Taskbar** space without a button | right click | Top, Bottom, Left, Right (`•` marks the current one), Larger, Smaller |
| Thin line along the **Taskbar** (`▁▔▏▕`, under windows) | drag | resize: 1–4 rows at the top or bottom, 12–40 columns at the left or right |

| **Machines Folder** row | double click, Enter | connect (see below); a connected **Machine** raises its windows |
| **Machines Folder** row | right click | Connect, Open ssh window (not connected); Show windows, Disconnect (connected) |

A top or bottom **Taskbar** wraps its **Task** buttons onto further rows; a left or right one
shows Start on the first row, one **Task** per row below, and the hint on the last row. Menus
opened from the bar open on the desk side of it; maximized windows follow the bar.

Menus: arrow keys move, Enter chooses, Esc closes. Dialogs: Enter confirms, Esc cancels.

## Keys

Ctrl+\ then: `n` new window · `w` close · `z` minimize · `m` maximize · `t` tile · `Tab` next ·
`q` quit · Ctrl+\ sends a literal Ctrl+\. Everything else goes to the focused window.

## Server state

- State is read over the public API socket: `session.snapshot` for **Tasks** and windows, and one
  `events.subscribe` stream per **Server** (workspace created, renamed, closed; tab created,
  closed, renamed, moved; pane created, closed). Each burst of events (100 ms) brings one new
  snapshot, so changes made by any other **Client** appear on the desk. An idle desk starts no
  `herdr` processes; only the ping of a connected **Machine** (below) runs `ssh -W` every 3 s.
- Changes use public CLI commands: `workspace create|rename|close`, `tab create|rename|close`,
  `pane run`; each window is one `terminal session control` child.
- A window shows `TabInfo.root_pane_id` (optional field added by sshdr). Against a server without
  it, the first pane without a label is used. Other panes in the tab are not shown; the title
  says `(+N panes)`.

## Machines

The **Machines Folder** lists Local, every enabled saved **Machine** (`herdr machine add`, read
from the endpoint catalog), and the **Machines** saved with `herdr machine add --ssh-only`
(`ssh-machines.json`, default Session); both files are read only. The **Tier** of every
**Machine** is detected on connect, whichever file it came from. Status per row: `Local`, `not connected`,
`connecting…`, `Server` (with its window count), `SSH`, `! auth`, `! offline`.

Connecting runs `herdr status server --json` on the **Machine** over SSH, which reports its API
socket, and detects the **Tier** each time (ADR 0004):

| Result | Status | Effect |
|---|---|---|
| Server running | `Server` | its workspaces join the **Taskbar** as `<machine>: <task>`; its tabs open as windows titled `<machine>: <tab>`; followed by events like Local |
| ssh exit 255 with an authentication or host-key error | `! auth` | nothing opens; no prompt (`BatchMode=yes`) |
| other ssh exit 255, or no answer in 12 s | `! offline` | nothing opens |
| ssh works, `herdr` is missing, fails, or its server is not running | `SSH` | a local window runs `ssh <target>`, in a local **Task** named after the **Machine** |

Remote commands run as `env HERDR_SESSION=<profile session> herdr …`, and API requests and the
event stream as `ssh -W <socket>`, over the managed ControlMaster (`platform::shared_ssh_control_path`,
the same socket herdr itself uses); no newer herdr is needed on the **Machine**. Each master
carries at most 8 window streams; more windows open another master, started before its streams.

A **Machine** without events for 3 s is pinged. A failed ping (3 s limit), snapshot (5 s), or a
closed event stream marks it `! offline` (shown in the **Taskbar** hint, within about 7 s of a
stall) and prefixes its window titles with `⚠`; windows keep their place, and the desk retries
every 2 s. A window whose stream ends stays put and asks for a snapshot: it reattaches in place,
or closes when its tab is gone. Disconnect stops following the **Machine** and closes its
windows; its tabs keep running.

## Saved layout

The desk writes `desk.json` in the Session data directory (`crate::session::data_dir()`) once the
layout has been unchanged for 1 s, and on quit (temporary file, then rename):
per window the position, size, maximize state, minimize state, and stacking order, keyed by the
terminal id (local) or `<profile id>/<terminal id>` (**Machine**), plus the **Machines** that were
connected, and the **Taskbar** edge and size (`taskbar`, omitted at the default bottom, 1 row).
The next start restores the layout and reconnects those **Machines**; a **Machine**'s windows get their saved place when they open. Windows without an entry open stacked on top.

## Limits

- Remote **Machines** need OpenSSH ControlMaster (Unix); the **Machines Folder** cannot add or
  remove **Machines** (`herdr machine add|remove`).
- Kitty keyboard protocol is not negotiated over `terminal session control` (legacy key bytes only).
- Not yet run on Windows (V2).
