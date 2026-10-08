# Backlog
Status: In progress

The one living tracker of open sshdr work. Rows move to **Done** with the closing commit; never
deleted. Evidence cites files and symbols, not line numbers.

## Open

| ID | Item | Evidence | Verified | Priority | Effort | Acceptance |
|---|---|---|---|---|---|---|
| B1 | Spec **Fleet Operations** (check, run, exec, cp, sync, snapshots) as one SSH implementation for every **Tier** over the managed OpenSSH config and ControlMaster (ADR 0004): which sshi logic is ported, fan-out and aggregation, on-demand connection | `~/repos/sshi` `src/commands/`, `ConcurrencyLimiter`; `src/remote/attach.rs` `write_managed_ssh_config`, `RemoteSsh`; ADR 0004 | 2026-10-08 | P1 | M | Approved spec in `docs/spec/` with chosen form and rejected alternatives |
| B2 | Spec **Remote Backup** derived from bkp (where archives live, pull vs. on-host tar, prune/restore over SSH) | `~/repos/bkp` `prune.rs`, `archive.rs` | 2026-10-06 | P2 | M | Approved spec in `docs/spec/`; depends on B1 |
| B3 | Rename the product from `herdr` to `sshdr` (binary, config/data paths, sockets, `HERDR_*` env, update and remote-install URLs). Owner direction (2026-10-08): convert fully to sshdr with no herdr interoperability. Open: whether **Upstream** sync continues (ADR 0001). Blast radius: `herdr` appears 3752 times in 237 `src/` files, 94 `HERDR_*` variables, `herdr.dev` in update and remote install | `Cargo.toml`; `src/session.rs`; `src/update.rs`; `src/remote/attach.rs` | 2026-10-08 | P1 | L | ADR recording rename scope and Upstream sync policy |
| B4 | Decide how Upstream CI/release workflows behave in the fork (disable, retarget, or keep) | `.github/workflows/` | 2026-10-06 | P2 | S | Workflows on `main` either pass or are intentionally disabled |
| B5 | Reconcile dependencies after ADR 0004: `russh`/`russh-sftp`/`SessionPool` likely dropped (transport is managed OpenSSH); still check `unicode-width` 0.1 vs 0.2, `chrono` vs `time`, `rusqlite` | sshi and herdr `Cargo.toml` | 2026-10-08 | P2 | S | Decision recorded in B1 spec |
| B7 | Add an sshdr fork notice to root `README.md` | `README.md` | 2026-10-06 | P3 | S | README states fork purpose and links `CONTEXT.md` |
| B8 | Decide the TUI modularization strategy (component kit, overlay trait, layout slots) and the Fleet/Backup UI form, from the TUI audit's open decisions. Overall TUI layout not yet designed (2026-10-07); depends on B9. Requirements drafted, owner still refining. Must include a connection status/control panel (ADR 0004) | `docs/spec/2026-10-07-tui-component-kit.md` (Draft); `docs/audit/2026-10-07-tui-components.md` §6–§7 | 2026-10-07 | P1 | M | Component kit spec approved; layout/UI-form spec approved |
| B9 | Decide where **Fleet** dispatch and aggregation live: in the **Client** (alongside the planned connection control panel), in the local **Server** (survives detach, reachable from CLI), or the CLI only | `docs/reference/server-client-remote.md` §4; `src/client/endpoint/supervisor.rs`; AGENTS.md runtime/client boundary guardrail; ADR 0004 | 2026-10-08 | P1 | S | Covered in B1 spec |
| B10 | Store **Fleet** data (**Groups**, shell type) in an sshdr-owned file keyed by profile id, not in `endpoints.json` | `src/client/endpoint/catalog.rs` `deny_unknown_fields`; `src/client/endpoint/ssh_metadata.rs`; ADR 0004 | 2026-10-08 | P1 | S | Covered in B1 spec |
| B11 | `machine add` saves an **SSH Tier** profile when herdr is absent or installation is declined, instead of discarding it | `src/cli/machine.rs`; ADR 0004 | 2026-10-08 | P1 | S | Covered in B1 spec |
| B12 | Classify connection failures into **Attention** (auth, host key) vs **SSH Tier** (no compatible **Server**); **SSH Tier** hosts connect on demand without heartbeat | `src/remote/saved.rs` `saved_ssh_failure_needs_attention`; `src/client/endpoint/supervisor.rs`; ADR 0004 | 2026-10-08 | P1 | M | Covered in B1 spec |

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
| B6 | Machine/Host relation: every **Host** is a **Machine** (ADR 0003, superseded by ADR 0004) | docs: record adr 0003, fleet hosts are machines |
