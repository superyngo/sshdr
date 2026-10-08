# sshdr Changelog

Changes made in sshdr on top of herdr. Upstream herdr changes are in `CHANGELOG.md`.

## [Unreleased]

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
