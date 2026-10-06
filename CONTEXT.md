# CONTEXT

Entry point for all sshdr documentation. sshdr is a fork of herdr (see
[`docs/reference/upstream-sync.md`](docs/reference/upstream-sync.md)). Upstream-owned files —
`CHANGELOG.md`, `docs/next/`, `docs/preview/`, `docs/versions/`, `CONTRIBUTING.md`, `SPONSORS.md` —
stay where Upstream put them ([ADR 0002](docs/adr/0002-docs-layout-coexists-with-upstream.md)).
sshdr changes are recorded in [`CHANGELOG.sshdr.md`](CHANGELOG.sshdr.md).

| Folder | Holds | Canonical? | Lifecycle |
|---|---|---|---|
| [`docs/reference/`](docs/reference/README.md) | Current behavior: glossary, per-subsystem contracts | Yes — the only source of truth | Kept in sync with the code |
| [`docs/adr/`](docs/adr/README.md) | Decisions that were expensive to reach and would be expensive to reverse | No — historical | Never edited; superseded by a new ADR |
| [`docs/spec/`](docs/spec/README.md) | Design records written before implementation | No — historical | Frozen once approved; only `Status:` changes |
| [`docs/plan/`](docs/plan/README.md) | Task-by-task implementation plans derived from a spec | No — historical | Frozen once shipped; only `Status:` changes |
| [`docs/plan/BACKLOG.md`](docs/plan/BACKLOG.md) | The one living tracker of open work, pending verification, external blockers, and watched items | No — live state | Never frozen while anything is open |
| [`docs/debug/`](docs/debug/README.md) | Handoff notes from investigations, with repro scripts | No — historical | Frozen once resolved; only `Status:` changes |
| [`docs/audit/`](docs/audit/README.md) | Point-in-time sweeps for bugs, dead code, inconsistency, plus assessment / verification runs | No — historical | Frozen once findings are addressed; only `Status:` changes |
| `docs/tmp/` | Scratch; `<agent>-scratch/` is gitignored | No | Archived to `tmp/archive/YYYY-MM.tar.gz` when stale |
| `docs/next/`, `docs/preview/`, `docs/versions/` | Upstream herdr user documentation | Upstream-owned | Follows Upstream |

## Reading order

1. [`docs/reference/glossary.md`](docs/reference/glossary.md) — the vocabulary every other file uses.
2. [`docs/reference/README.md`](docs/reference/README.md) — the subsystem map.
3. [`docs/adr/README.md`](docs/adr/README.md) — why the shape is what it is.
4. [`CHANGELOG.sshdr.md`](CHANGELOG.sshdr.md) — what changed recently in sshdr.
5. [`docs/plan/BACKLOG.md`](docs/plan/BACKLOG.md) — what is still open.
