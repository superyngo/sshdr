# sshdr Changelog

Changes made in sshdr on top of herdr. Upstream herdr changes are in `CHANGELOG.md`.

## [Unreleased]

### 2026-10-25

- feat: `machine add --ssh-only` saves a host without herdr setup (B11) in sshdr's
  `client/ssh-machines.json`; a failed setup with working SSH suggests it; `machine list`
  (`ssh_only`), `rename`, and `remove` include these machines.
- feat: desk lists ssh-only machines in the Machines Folder; their Tier is detected on connect.
- feat: desk Taskbar on any screen edge (B21): drag its empty space or use its right-click menu;
  resize it by dragging the thin line beside it (1–4 rows, 12–40 columns); saved in `desk.json`.
- feat: desk follows server events instead of polling (B23): one `events.subscribe` stream per
  Server, local over the API socket and remote over `ssh -W`; an idle desk starts no `herdr`
  processes, a quiet Machine is pinged every 3 s.
- feat: desk saves `desk.json` 1 s after the layout settles, so a killed desk keeps its layout.
- feat: desk Machines Folder (B15): `▣ Machines` icon and Start Menu entry list Local and saved
  Machines with their Tier; Server Tier Machines add their Tasks and windows over SSH, SSH Tier
  opens an `ssh` window, Attention shows `! auth`; connected Machines reconnect on start.
- feat: desk targets for local and remote Machines: per-Machine polling with a 5 s timeout
  (`! offline`), windows kept and reattached in place across stream loss, at most 8 streams per
  ControlMaster with the master started before its streams.
- docs: plan b15 machines folder (desktop icon, per-Machine Tier, remote Tasks, stream budget,
  windows kept across disconnects).
- docs: backlog adds the owner's Desktop suggestions: minimize into Taskbar groups, right-click
  rename menus, and New/Quit under the Start Menu (B14); hierarchy mapping decision (B20); movable,
  resizable Taskbar (B21).
- docs: ADR 0007 maps the Desktop onto herdr: Tasks are workspaces, Terminal Windows are tabs
  (root pane), and remote Server Tier tabs open as their own Windows over SSH (amends ADR 0006
  decisions 2 and 4); glossary adds Task and Minimize; backlog closes B20, updates B14 and B15,
  adds the B22 remote-stream spike.
- docs: plan B14 desk shell: Tasks from workspaces, Windows from tab root panes, Taskbar,
  Start Menu, Minimize, context-menu rename, and `desk.json` window state; adds optional
  `root_pane_id` to `TabInfo`; Windows stays in V2.
- feat: `herdr desk` client from the B13 spike lands on branch `b14-desk-shell`: overlapping
  terminal windows over `terminal session control` streams; Windows VT console input moves to
  `crate::platform::enable_virtual_terminal_input`.
- feat: desk Tasks and windows come from the server: every workspace is a Task on the Taskbar
  (label and window count, click raises its top window), every tab a window on its root pane
  (title notes extra panes); a 1 s poll follows tabs created, renamed, or closed elsewhere.
  `TabInfo` gains optional `root_pane_id` (API schema regenerated).
- feat: desk windows minimize (`[_]`, Ctrl+\ `z`) into their Task; a left click on a Task lists
  its windows (minimized ones marked) and restores the chosen one; right-click menus on window
  titles (rename, minimize, maximize, close) and Task buttons (rename, new window, minimize or
  restore all, close task with confirmation); renames write back with `tab rename` /
  `workspace rename`. Menu and dialog ported from wens-tui-lab.
- feat: desk Start Menu: a `⊞ Start` button at the left of the Taskbar opens New window, New
  task (`workspace create`), Tile all, and Quit desk; replaces the spike's New/Tile/Quit buttons.
- feat: desk saves window position, size, maximize, minimize, and stacking order to
  `desk.json` in the Session data directory on quit and restores them on the next start;
  windows without saved state open stacked on top.
- docs: record B14 desk shell results (per-step live checks, 1 vs 15 window cost against B13)
  in the plan; add `docs/reference/desk.md`; backlog moves B14 to Done and adds B23 (save on
  change, event-driven sync).
- docs: plan B22 remote desk spike (desk over SSH against omarchy in an isolated Session,
  shared ControlMaster, latency, stream cost, disconnect, Attention).
- docs: record B22 remote desk spike results: remote windows over SSH work against herdr 0.9.5
  (+13 ms typing latency, ~25 KiB/s per master); sshd `MaxSessions` 10 caps streams per
  ControlMaster, disconnects drop window geometry, polls need a timeout. Backlog closes B22 and
  carries the findings into B15.

### 2026-10-24

- docs: ADR 0006 turns sshdr into a TUI Desktop built as a separate Client (`herdr desk`) whose
  Terminal Windows are local Server panes; assessment in
  `docs/audit/2026-10-24-desktop-env-assessment.md`; glossary rewrites sshdr and adds Desktop terms;
  backlog defers Fleet and Remote Backup (B1, B2, B5, B9, B10), rescopes B8, adds B13-B19.
- docs: B13 desk spike plan (`docs/plan/2026-10-24-b13-desk-spike.md`).
- docs: B13 desk spike results (`docs/audit/2026-10-24-b13-desk-spike.md`): terminal windows over
  session streams work on macOS (code on branch `spike/b13-desk`); backlog closes B13 and V1, adds V2
  (Windows run), and carries spike findings into B14.

### 2026-10-08

- docs: ADR 0004 supersedes 0003: Fleet Hosts are registered Machines with a connect-time Tier
  (SSH or Server), one SSH implementation of Fleet Operations, on-demand connections; glossary adds
  Tier and rewrites Machine/Host/Fleet/Fleet Operation; backlog re-scopes B1/B3/B5/B8/B9, adds
  B10-B12.
- docs: ADR 0005 keeps syncing Upstream and defers the sshdr rename to a later hard-fork point;
  backlog B3 now tracks the hard-fork criteria.
- docs: backlog sequences B8 (TUI/component design) before B1 and B9.

### 2026-10-07

- docs: audit herdr TUI components and their modularity (`docs/audit/2026-10-07-tui-components.md`);
  backlog B8 tracks the resulting strategy decision.
- docs: reference for server/client responsibilities and remote connection paths
  (`docs/reference/server-client-remote.md`); glossary adds Bridge, Shell Snapshot, Pane Surface;
  backlog B9 tracks where Fleet runtime lives.
- merge: sync Upstream to `a124eed7` (pane key encoding routed through libghostty, #5017).
- docs: draft TUI Component Kit requirements (`docs/spec/2026-10-07-tui-component-kit.md`);
  glossary adds Component Kit; backlog records the every-Host-is-a-Machine decision (B6) and the
  unverified sync build (V1).
- docs: ADR 0003 makes every Fleet Host a herdr Machine; glossary Fleet/Host/Group/Fleet Operation
  rewritten; backlog B1/B5/B9 re-scoped, B6 done.

### 2026-10-06

- docs: bootstrap sshdr fork documentation layout (`CONTEXT.md`, glossary, ADRs 0001–0002,
  backlog, upstream sync procedure); `main` becomes the Mainline and `master` mirrors Upstream.
