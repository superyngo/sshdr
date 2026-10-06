# 0001 — Fork branch model

Date: 2026-10-06

## Context

sshdr is a GitHub fork of `herdrdev/herdr` that will diverge substantially (**Fleet** management,
**Remote Backup**) while continuing to take **Upstream** fixes.

## Decision

- `master` is the **Mirror Branch**: it tracks `upstream/master`, moves only by fast-forward, and
  is pushed to `origin` unchanged.
- `main` is the **Mainline** and the GitHub default branch. All sshdr work lands there.
- **Upstream** changes enter `main` by merging `master`, never by rebasing `main`.

Procedure: [`../reference/upstream-sync.md`](../reference/upstream-sync.md).

## Alternatives rejected

- **Work directly on `master`**: loses a clean view of **Upstream** and makes every sync a
  conflict-laden merge onto the same branch name GitHub uses for the fork relationship.
- **Rebase `main` onto Upstream**: rewrites published history; unworkable once `main` is shared.
- **Keep `master` as default branch**: visitors and PRs would land on an unmodified herdr.
