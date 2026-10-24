# Architecture Decision Records

One file per decision that was expensive to reach and would be expensive to reverse. An ADR
records *why* and which alternatives were rejected; it is a historical record, never edited.
Current behavior lives in [`../reference/`](../reference/README.md).

| # | Decision | Status |
|---|---|---|
| [0001](0001-fork-branch-model.md) | `master` mirrors Upstream; `main` is the Mainline and default branch | Implemented (2026-10-06) |
| [0002](0002-docs-layout-coexists-with-upstream.md) | sshdr docs layout coexists with Upstream-owned docs and root files | Implemented (2026-10-06) |
| [0003](0003-fleet-hosts-are-machines.md) | Every Fleet Host is a herdr Machine; Fleet Operations run through herdr Servers | Superseded by 0004 (2026-10-08) |
| [0004](0004-fleet-registered-machines-with-connect-time-tiers.md) | Fleet Hosts are registered Machines; Tier (SSH or Server) detected on each connection; one SSH implementation of Fleet Operations | Accepted (2026-10-08) |
| [0005](0005-sync-now-hard-fork-later.md) | Keep syncing Upstream; rename to sshdr in one pass at a later hard-fork point | Accepted (2026-10-08) |
| [0006](0006-tui-desktop-as-separate-client.md) | sshdr becomes a TUI Desktop: a separate Client (`herdr desk`) whose Terminal Windows are local Server panes; Fleet and Remote Backup deferred | Accepted (2026-10-24) |
