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
| B13 | P0 spike: 2–3 local panes shown as **Terminal Windows** through `terminal session control` streams with client-side ANSI parsing; drag and resize; keyboard and mouse forwarding; detach and reattach; macOS, Linux, Windows | `src/client/terminal_sessions.rs`; `ClientMessage::ControlTerminal`; `crates/ghostty-vt`; ADR 0006 | 2026-10-24 | P1 | M | Pty-driven run of the real binary passes; render cost with 1 vs 15 **Windows** reported; Windows go/no-go recorded |
| B14 | P1 **Desktop** shell: `herdr desk`, **Windows**, **Taskbar**, **Start Menu**, **Snap**, persisted **Window** geometry | `~/repos/wens-tui-lab/src/desk.rs`, `desk_input.rs`; ADR 0006 | 2026-10-24 | P1 | M | Buffer and hit-map tests; pty-driven run; depends on B13 |
| B15 | P2 **Machines Folder**: desktop icon, list/icon view, Tier status, double-click connects (nested herdr **Client** at **Server Tier**, `ssh` at **SSH Tier**) | `src/client/endpoint/catalog.rs` `EndpointCatalog`; ADR 0004, 0006 | 2026-10-24 | P1 | S–M | Connects to a real **Machine** at each Tier; depends on B14 |
| B16 | P3 local file manager: multi-select, drag-and-drop, cut/copy/paste, **File Clipboard**, text clipboard, **File Associations** | `selection.rs`; `platform::read_clipboard_text`; herdr-sidebar `actions.rs` (single editor command only) | 2026-10-24 | P1 | L | Unit and pty-driven tests; depends on B14 |
| B17 | P4 remote file manager: SSH file layer over the managed OpenSSH config (a subset of sshi `cp`), copy between **Machines**, progress, conflicts, cancel; Windows hosts | `src/remote/attach.rs` `RemoteSsh`, `write_managed_ssh_config`; `~/repos/sshi`; ADR 0004, 0006 | 2026-10-24 | P1 | L | Verified against real remote **Machines**; depends on B16 |
| B18 | P5 **App** model: herdr plugin manifest plus **Desktop** fields (icon, **Start Menu** entry, **File Associations**, window placement) | `src/api/schema/plugins.rs`; `src/app/api/plugins/`; ADR 0006 | 2026-10-24 | P2 | M | Sample **App** installs and launches from the **Start Menu**; depends on B14 |
| B19 | Touch layout for the **Desktop** (tap = press + release; no drag, right-click, or long-press) | `~/repos/wens-tui-lab/src/mobile.rs`; tui-components contract C12; ADR 0006 | 2026-10-24 | P3 | L | Spec approved; after the first release |

## Pending verification

| ID | Item | How to verify |
|---|---|---|
| V1 | Upstream sync to `a124eed7` merged into `main` without conflicts; build not verified because Zig 0.16.0 is not installed on this workstation (`crates/ghostty-vt/build.rs` panics) | Install Zig 0.16.0, then `just check` on `main` |

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
| B6 | Machine/Host relation: every **Host** is a **Machine** (ADR 0003, superseded by ADR 0004) | docs: record adr 0003, fleet hosts are machines |
