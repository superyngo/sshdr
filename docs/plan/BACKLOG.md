# Backlog
Status: In progress

The one living tracker of open sshdr work. Rows move to **Done** with the closing commit; never
deleted. Evidence cites files and symbols, not line numbers.

## Open

| ID | Item | Evidence | Verified | Priority | Effort | Acceptance |
|---|---|---|---|---|---|---|
| B1 | Spec **Fleet Operations** as herdr API methods run by each **Host**'s **Server** (ADR 0003): which sshi logic (check, run, exec, cp, sync, snapshots) moves into the server, how the local side fans out and aggregates, and how sshi code is brought in | `~/repos/sshi` `src/commands/`, `ConcurrencyLimiter`; `src/cli/target.rs`, `src/remote/saved.rs` `SavedSshApiBridge`; ADR 0003 | 2026-10-07 | P1 | M | Approved spec in `docs/spec/` with chosen form and rejected alternatives |
| B2 | Spec **Remote Backup** derived from bkp (where archives live, pull vs. on-host tar, prune/restore over SSH) | `~/repos/bkp` `prune.rs`, `archive.rs` | 2026-10-06 | P2 | M | Approved spec in `docs/spec/`; depends on B1 |
| B3 | Decide whether the binary/product is renamed from `herdr` to `sshdr` (Cargo, paths, sockets, update channels) | `Cargo.toml` `name = "herdr"`; `src/update.rs` | 2026-10-06 | P2 | L | ADR recording the decision |
| B4 | Decide how Upstream CI/release workflows behave in the fork (disable, retarget, or keep) | `.github/workflows/` | 2026-10-06 | P2 | S | Workflows on `main` either pass or are intentionally disabled |
| B5 | Reconcile dependencies after ADR 0003: `russh`/`russh-sftp`/`SessionPool` likely unneeded (transport is the herdr **Bridge**); still check `unicode-width` 0.1 vs 0.2, `chrono` vs `time`, `rusqlite` | sshi and herdr `Cargo.toml` | 2026-10-07 | P2 | S | Decision recorded in B1 spec |
| B7 | Add an sshdr fork notice to root `README.md` | `README.md` | 2026-10-06 | P3 | S | README states fork purpose and links `CONTEXT.md` |
| B8 | Decide the TUI modularization strategy (component kit, overlay trait, layout slots) and the Fleet/Backup UI form, from the TUI audit's open decisions. Overall TUI layout not yet designed (2026-10-07); depends on B9. Requirements drafted, owner still refining | `docs/spec/2026-10-07-tui-component-kit.md` (Draft); `docs/audit/2026-10-07-tui-components.md` §6–§7 | 2026-10-07 | P1 | M | Component kit spec approved; layout/UI-form spec approved |
| B9 | Decide where **Fleet** dispatch and aggregation live locally (execution is on each **Host**'s **Server** per ADR 0003): in the **Client** (like `EndpointSupervisors`), in the local **Server** (survives detach, reachable from CLI), or the CLI only | `docs/reference/server-client-remote.md` §4; `src/client/endpoint/supervisor.rs`; AGENTS.md runtime/client boundary guardrail | 2026-10-07 | P1 | S | Covered in B1 spec |

## Pending verification

| ID | Item | How to verify |
|---|---|---|
| V1 | Upstream sync to `a124eed7` merged into `main` without conflicts; build not verified because Zig 0.16.0 is not installed on this workstation (`crates/ghostty-vt/build.rs` panics) | Install Zig 0.16.0, then `just check` on `main` |

## Awaiting external

None.

## Watching

None.

## Done

| ID | Item | Commit |
|---|---|---|
| B6 | Machine/Host relation: every **Host** is a **Machine** (ADR 0003) | docs: record adr 0003, fleet hosts are machines |
