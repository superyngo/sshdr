# Backlog
Status: In progress

The one living tracker of open sshdr work. Rows move to **Done** with the closing commit; never
deleted. Evidence cites files and symbols, not line numbers.

## Open

| ID | Item | Evidence | Verified | Priority | Effort | Acceptance |
|---|---|---|---|---|---|---|
| B3 | Decide the hard-fork point (criteria or milestone) at which Upstream sync stops and everything is renamed to sshdr in one pass (ADR 0005). Rename scope: `herdr` 3752 times in 237 `src/` files, 94 `HERDR_*` variables, `herdr.dev` update and remote-install URLs | `Cargo.toml`; `src/session.rs`; `src/update.rs`; `src/remote/attach.rs`; ADR 0005 | 2026-10-08 | P2 | L | Hard-fork criteria recorded; rename plan in `docs/plan/` |
| B4 | Decide how Upstream CI/release workflows behave in the fork (disable, retarget, or keep) | `.github/workflows/` | 2026-10-06 | P2 | S | Workflows on `main` either pass or are intentionally disabled |
| B7 | Add an sshdr fork notice to root `README.md` | `README.md` | 2026-10-06 | P3 | S | README states fork purpose and links `CONTEXT.md` |
| B8 | Rescoped by ADR 0006 (2026-10-24): design the **Desktop**'s own component set (hit map, menu, window/**Snap** geometry, list, dialog), lifted from `~/repos/wens-tui-lab` (`desk.rs`, `hitmap.rs`, `menu.rs`, `geom.rs`). The shell-overlay hook-point constraint (spec R10) no longer applies because the **Desktop** is a separate **Client** | `docs/spec/2026-10-07-tui-component-kit.md` (Draft); `docs/audit/2026-10-24-desktop-env-assessment.md`; ADR 0006 | 2026-10-24 | P1 | M | Component spec for the **Desktop** approved |
| B11 | `machine add` saves an **SSH Tier** profile when herdr is absent or installation is declined, instead of discarding it. Needed by the **Machines Folder** (B15) | `src/cli/machine.rs`; ADR 0004 | 2026-10-24 | P1 | S | **SSH Tier** profile persisted and listed |
| B12 | Classify connection failures into **Attention** (auth, host key) vs **SSH Tier** (no compatible **Server**); **SSH Tier** hosts connect on demand without heartbeat. Needed by the **Machines Folder** (B15) | `src/remote/saved.rs` `saved_ssh_failure_needs_attention`; `src/client/endpoint/supervisor.rs`; ADR 0004 | 2026-10-24 | P1 | M | Tier shown per **Machine** |
| B15 | P2 **Machines Folder**: desktop icon, list/icon view, Tier status; opening a **Server Tier** **Machine** shows its **Tasks** and opens its tabs as **Terminal Windows** over SSH (ADR 0007); an **SSH Tier** **Machine** opens `ssh` | `src/client/endpoint/catalog.rs` `EndpointCatalog`; ADR 0004, 0006, 0007; B22 results `docs/audit/2026-10-25-b22-remote-desk-spike.md` §5 (max 9 streams per ControlMaster, keep windows across disconnects, poll timeout, share herdr's master) | 2026-10-25 | P1 | M | Connects to a real **Machine** at each Tier; 15 remote windows open no unmanaged direct TCP connections; a dropped link keeps window geometry |
| B16 | P3 local file manager: multi-select, drag-and-drop, cut/copy/paste, **File Clipboard**, text clipboard, **File Associations** | `selection.rs`; `platform::read_clipboard_text`; herdr-sidebar `actions.rs` (single editor command only) | 2026-10-24 | P1 | L | Unit and pty-driven tests; depends on B14 |
| B17 | P4 remote file manager: SSH file layer over the managed OpenSSH config (a subset of sshi `cp`), copy between **Machines**, progress, conflicts, cancel; Windows hosts | `src/remote/attach.rs` `RemoteSsh`, `write_managed_ssh_config`; `~/repos/sshi`; ADR 0004, 0006 | 2026-10-24 | P1 | L | Verified against real remote **Machines**; depends on B16 |
| B18 | P5 **App** model: herdr plugin manifest plus **Desktop** fields (icon, **Start Menu** entry, **File Associations**, window placement) | `src/api/schema/plugins.rs`; `src/app/api/plugins/`; ADR 0006 | 2026-10-24 | P2 | M | Sample **App** installs and launches from the **Start Menu**; depends on B14 |
| B19 | Touch layout for the **Desktop** (tap = press + release; no drag, right-click, or long-press) | `~/repos/wens-tui-lab/src/mobile.rs`; tui-components contract C12; ADR 0006 | 2026-10-24 | P3 | L | Spec approved; after the first release |
| B21 | Movable **Taskbar**: drag it to the top, bottom, left, or right edge; drag its inner border to change its height or width; position and size persist | Owner suggestion 2026-10-25; `~/repos/wens-tui-lab/src/desk.rs` (bottom bar only) | 2026-10-25 | P3 | M | Hit-map tests for each edge; pty-driven run; depends on B14 |
| B23 | **Desktop** follow-ups from B14: save `desk.json` on change, not only on quit; replace the 1 s three-command poll with `events.subscribe` | `src/cli/desk/app.rs` `run`; `src/cli/desk/server.rs` `spawn_poller`; plan 2026-10-25 §8 | 2026-10-25 | P3 | S | Killing the desk keeps the last layout; idle desk spawns no CLI children |

## Pending verification

| ID | Item | How to verify |
|---|---|---|
| V2 | `herdr desk` on Windows: the `#[cfg(windows)]` VT-input path has never compiled or run (B13 skipped Windows) | `just setup-windows-cross` (owner accepts the Microsoft SDK license), `just windows-lint`, then run on ezpad |

## Awaiting external

None.

## Watching

Deferred by ADR 0006 (2026-10-24); resume when **Fleet** and **Remote Backup** return as **Apps**.

| ID | Item | Evidence | Verified |
|---|---|---|---|
| B1 | Spec **Fleet Operations** (check, run, exec, cp, sync, snapshots) as one SSH implementation for every **Tier** over the managed OpenSSH config and ControlMaster (ADR 0004). The `cp` subset moves to B17 | `~/repos/sshi` `src/commands/`, `ConcurrencyLimiter`; `src/remote/attach.rs` `write_managed_ssh_config`, `RemoteSsh`; ADR 0004 | 2026-10-08 |
| B2 | Spec **Remote Backup** derived from bkp (where archives live, pull vs. on-host tar, prune/restore over SSH) | `~/repos/bkp` `prune.rs`, `archive.rs` | 2026-10-06 |
| B5 | Reconcile dependencies after ADR 0004: `russh`/`russh-sftp`/`SessionPool` likely dropped (transport is managed OpenSSH); still check `unicode-width` 0.1 vs 0.2, `chrono` vs `time`, `rusqlite` | sshi and herdr `Cargo.toml` | 2026-10-08 |
| B9 | Decide where **Fleet** dispatch and aggregation live: in the **Client**, in the local **Server** (survives detach, reachable from CLI), or the CLI only | `docs/reference/server-client-remote.md` §4; AGENTS.md runtime/client boundary guardrail; ADR 0004 | 2026-10-08 |
| B10 | Store **Fleet** data (**Groups**, shell type) in an sshdr-owned file keyed by profile id, not in `endpoints.json` | `src/client/endpoint/catalog.rs` `deny_unknown_fields`; ADR 0004 | 2026-10-08 |

## Done

| ID | Item | Commit |
|---|---|---|
| B22 | Spike: remote **Terminal Windows** over SSH on omarchy (herdr 0.9.5): works; limits found (sshd `MaxSessions` 10, windows lost on disconnect, no poll timeout); results `docs/audit/2026-10-25-b22-remote-desk-spike.md`, code on `spike/b22-remote-desk` | docs: record b22 remote desk spike results |
| B14 | P1 **Desktop** shell: **Tasks** from workspaces, **Terminal Windows** on tab root panes, **Taskbar**, **Start Menu**, **Minimize**, right-click rename, saved layout; plan `2026-10-25-b14-desk-shell.md`, reference `docs/reference/desk.md` | docs: record b14 desk shell |
| B20 | **Desktop** hierarchy: **Task** = workspace, **Terminal Window** = tab (root pane), remote **Server Tier** tabs open as their own **Windows** over SSH (ADR 0007) | docs: record adr 0007, desktop tasks are workspaces and remote tabs are windows |
| B13 | P0 spike: **Terminal Windows** over `terminal session control` streams; interaction, reattach, and 1 vs 15 window cost verified on macOS; Windows skipped (V2) | docs: record b13 desk spike results |
| V1 | Upstream sync `a124eed7` verified: `just check` on `main` passes except `windows-lint` (no SDK); maintenance tests need Python 3.11+ (`just --set python`) | docs: record b13 desk spike results |
| B6 | Machine/Host relation: every **Host** is a **Machine** (ADR 0003, superseded by ADR 0004) | docs: record adr 0003, fleet hosts are machines |
