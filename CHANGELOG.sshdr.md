# sshdr Changelog

Changes made in sshdr on top of herdr. Upstream herdr changes are in `CHANGELOG.md`.

## [Unreleased]

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

### 2026-10-06

- docs: bootstrap sshdr fork documentation layout (`CONTEXT.md`, glossary, ADRs 0001–0002,
  backlog, upstream sync procedure); `main` becomes the Mainline and `master` mirrors Upstream.
