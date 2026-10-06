# Upstream sync

How sshdr tracks **Upstream** (`herdrdev/herdr`). Decision record:
[ADR 0001](../adr/0001-fork-branch-model.md).

## Remotes and branches

| Name | Points at | Role |
|---|---|---|
| `origin` | `superyngo/sshdr` | sshdr on GitHub |
| `upstream` | `herdrdev/herdr` | **Upstream** |
| `master` | tracks `upstream/master` | **Mirror Branch**: fast-forward only, no sshdr commits |
| `main` | tracks `origin/main` | **Mainline** and GitHub default branch |

## Sync procedure

```sh
git fetch upstream
git switch master && git merge --ff-only upstream/master && git push origin master
git switch main && git merge master      # merge, never rebase: main is published
```

Resolve conflicts on `main`. Upstream-owned files (`CHANGELOG.md`, `docs/next/`, `docs/preview/`,
`docs/versions/`, root `README.md`) take the **Upstream** side unless sshdr deliberately changed
them; sshdr changes go in `CHANGELOG.sshdr.md`.

## Contributing upstream

The sshdr owner is not a herdr maintainer. The external contributor guardrail in `AGENTS.md` applies
to any interaction with `herdrdev/herdr`.
