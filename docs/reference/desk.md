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

| **Machines Folder** row | double click, Enter | connect (see below); a connected **Machine** raises its windows |
| **Machines Folder** row | right click | Connect, Open ssh window (not connected); Show windows, Disconnect (connected) |

Menus: arrow keys move, Enter chooses, Esc closes. Dialogs: Enter confirms, Esc cancels.

## Keys

Ctrl+\ then: `n` new window · `w` close · `z` minimize · `m` maximize · `t` tile · `Tab` next ·
`q` quit · Ctrl+\ sends a literal Ctrl+\. Everything else goes to the focused window.

## Server state

- Only public CLI commands are used: `workspace list|create|rename|close`,
  `tab list|create|rename|close`, `pane list`, and one `terminal session control` child per window.
- The desk polls the three lists every second, so workspaces and tabs created, renamed, or closed
  by any other **Client** appear on the desk.
- A window shows `TabInfo.root_pane_id` (optional field added by sshdr). Against a server without
  it, the first pane without a label is used. Other panes in the tab are not shown; the title
  says `(+N panes)`.

## Machines

The **Machines Folder** lists Local and every enabled saved **Machine** (`herdr machine add`;
read from the endpoint catalog, never written). Status per row: `Local`, `not connected`,
`connecting…`, `Server` (with its window count), `SSH`, `! auth`, `! offline`.

Connecting runs `herdr workspace list` on the **Machine** over SSH and detects the **Tier** each
time (ADR 0004):

| Result | Status | Effect |
|---|---|---|
| Lists returned | `Server` | its workspaces join the **Taskbar** as `<machine>: <task>`; its tabs open as windows titled `<machine>: <tab>`; polled every second like Local |
| ssh exit 255 with an authentication or host-key error | `! auth` | nothing opens; no prompt (`BatchMode=yes`) |
| other ssh exit 255, or no answer in 12 s | `! offline` | nothing opens |
| ssh works, `herdr` fails or is missing | `SSH` | a local window runs `ssh <target>`, in a local **Task** named after the **Machine** |

Remote commands run as `env HERDR_SESSION=<profile session> herdr …` over the managed
ControlMaster (`platform::shared_ssh_control_path`, the same socket herdr itself uses). Each master
carries at most 8 window streams; more windows open another master, started before its streams.

A poll that fails or takes over 5 s marks the **Machine** `! offline` (shown in the **Taskbar**
hint) and prefixes its window titles with `⚠`; windows keep their place. A window whose stream
ends stays put and reattaches after the next successful poll; it closes only when its tab is gone.
Disconnect stops polling and closes the **Machine**'s windows; its tabs keep running.

## Saved layout

On quit the desk writes `desk.json` in the Session data directory (`crate::session::data_dir()`):
per window the position, size, maximize state, minimize state, and stacking order, keyed by the
terminal id (local) or `<profile id>/<terminal id>` (**Machine**), plus the **Machines** that were
connected. The next start restores the layout and reconnects those **Machines**; a **Machine**'s
windows get their saved place when they open. Windows without an entry open stacked on top.

## Limits

- Remote **Machines** need OpenSSH ControlMaster (Unix); the **Machines Folder** cannot add or
  remove **Machines** (`herdr machine add|remove`).
- `machine add` does not yet save **SSH Tier** profiles (B11), so only hand-edited catalogs list them.
- Kitty keyboard protocol is not negotiated over `terminal session control` (legacy key bytes only).
- Not yet run on Windows (V2).
