# 0005 — Sync with Upstream now, hard-fork and rename later

Date: 2026-10-08

## Context

The owner's goal is a full conversion to sshdr with no herdr interoperability. Renaming touches
`herdr` 3752 times in 237 `src/` files, 94 `HERDR_*` variables, and the `herdr.dev` update and
remote-install URLs. **Upstream** is very active (461 commits in 60 days) and still delivers fixes
to the terminal runtime, agent detection, and Windows support.

## Decision

1. Keep the ADR 0001 sync model (**Mirror Branch** merged into **Mainline**) until a deliberate
   hard-fork point.
2. Until then, keep herdr identifiers, paths, sockets, environment variables, and URLs. sshdr work is
   additive: new modules and files, with small, listed hook points in **Upstream** files.
3. At the hard-fork point, stop syncing, retire ADR 0001's **Mirror Branch**, and rename everything
   to sshdr in one pass, including dropping compatibility with herdr **Servers**.

When to hard-fork is not yet decided; it is tracked in the backlog (B3).

## Alternatives rejected

- **Rename now, keep syncing**: most syncs would conflict across the renamed files.
- **Hard-fork now**: loses **Upstream** fixes while sshdr's own work is still early.
