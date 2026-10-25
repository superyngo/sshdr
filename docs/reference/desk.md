# Desktop client (`herdr desk`)

The **Desktop** is a separate **Client** (ADR 0006) that shows the local **Server**'s Session as
overlapping **Terminal Windows**. Mapping (ADR 0007): every workspace is a **Task**, every tab a
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
| `⊞ Start` | click | **Start Menu**: New window, New task, Tile all, Quit desk |
| **Task** button | click | the Task's windows (`▁` = minimized); choosing one restores and raises it |
| **Task** button | right click | Rename…, New window, Minimize all, Restore all, Close task… (confirms) |

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

## Saved layout

On quit the desk writes `desk.json` in the Session data directory (`crate::session::data_dir()`):
per terminal id the window position, size, maximize state, minimize state, and stacking order.
The next start restores them; windows without an entry open stacked on top.

## Limits

- Local **Server** only; remote **Machines** are B22 / B15.
- Kitty keyboard protocol is not negotiated over `terminal session control` (legacy key bytes only).
- Not yet run on Windows (V2).
