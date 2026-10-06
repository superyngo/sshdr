# Backlog
Status: In progress

The one living tracker of open sshdr work. Rows move to **Done** with the closing commit; never
deleted. Evidence cites files and symbols, not line numbers.

## Open

| ID | Item | Evidence | Verified | Priority | Effort | Acceptance |
|---|---|---|---|---|---|---|
| B1 | Spec how sshi is integrated (vendored workspace crate, git submodule, or external plugin) to provide **Fleet Operations** | `~/repos/sshi` `SessionPool`, `ConcurrencyLimiter`; herdr `src/cli.rs` | 2026-10-06 | P1 | M | Approved spec in `docs/spec/` with chosen form and rejected alternatives |
| B2 | Spec **Remote Backup** derived from bkp (where archives live, pull vs. on-host tar, prune/restore over SSH) | `~/repos/bkp` `prune.rs`, `archive.rs` | 2026-10-06 | P2 | M | Approved spec in `docs/spec/`; depends on B1 |
| B3 | Decide whether the binary/product is renamed from `herdr` to `sshdr` (Cargo, paths, sockets, update channels) | `Cargo.toml` `name = "herdr"`; `src/update.rs` | 2026-10-06 | P2 | L | ADR recording the decision |
| B4 | Decide how Upstream CI/release workflows behave in the fork (disable, retarget, or keep) | `.github/workflows/` | 2026-10-06 | P2 | S | Workflows on `main` either pass or are intentionally disabled |
| B5 | Reconcile dependencies: `unicode-width` 0.1 (sshi) vs 0.2 (herdr); `chrono` vs `time`; new `rusqlite`, `russh`, `russh-sftp` | sshi and herdr `Cargo.toml` | 2026-10-06 | P2 | S | Decision recorded in B1 spec |
| B6 | Define the relation between **Machine** (`EndpointCatalog`) and **Host** (sshi `HostEntry`) records | `src/client/endpoint/catalog.rs`; sshi `config/schema.rs` | 2026-10-06 | P1 | S | Covered in B1 spec |
| B7 | Add an sshdr fork notice to root `README.md` | `README.md` | 2026-10-06 | P3 | S | README states fork purpose and links `CONTEXT.md` |

## Pending verification

None.

## Awaiting external

None.

## Watching

None.

## Done

| ID | Item | Commit |
|---|---|---|
