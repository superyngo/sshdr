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

## GitHub Actions

Actions are turned off for `superyngo/sshdr` in repository settings (B4), so **Upstream**
workflows never run in the fork (pushing `master` or a `v*`/`preview-*` tag would trigger them).
The workflow files stay unchanged, so syncs never conflict over them. Check or reverse with:

```sh
gh api repos/superyngo/sshdr/actions/permissions
gh api -X PUT repos/superyngo/sshdr/actions/permissions -F enabled=true   # turn back on
```

Run checks locally instead (`just ci`). The checkout's `gh` default repository is
`superyngo/sshdr` (`gh repo set-default`); without it `gh` resolved to **Upstream**.

## Contributing upstream

The sshdr owner is not a herdr maintainer. The external contributor guardrail in `AGENTS.md` applies
to any interaction with `herdrdev/herdr`.
