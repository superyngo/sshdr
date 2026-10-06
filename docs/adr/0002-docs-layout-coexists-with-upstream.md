# 0002 — Docs layout coexists with Upstream

Date: 2026-10-06

## Context

wens-dev-principles docs 1 limits the repo root to `README.md`, `CHANGELOG.md`, `CONTEXT.md`, and
`LICENSE`; docs 2 requires `docs/` to hold exactly `reference/`, `adr/`, `spec/`, `plan/`, `debug/`,
`audit/`, `tmp/`. **Upstream** owns `docs/next/`, `docs/preview/`, `docs/versions/`, root
`CHANGELOG.md`, and several root files (`AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `SPONSORS.md`,
`README.zh-CN.md`). Its release CI and scripts depend on those paths, and moving them would conflict
on every sync. The global agent rule to append to root `CHANGELOG.md` conflicts with Upstream's
release-curated changelog in the same way.

## Decision

Deviate from wens-dev-principles docs 1 and docs 2:

1. Keep Upstream's `docs/next/`, `docs/preview/`, `docs/versions/` beside the seven standard
   folders. They are Upstream's user documentation, not sshdr reference.
2. Keep Upstream's root files. `AGENTS.md` gains only a short sshdr section at the top that points
   at `CONTEXT.md`.
3. Record sshdr changes in root `CHANGELOG.sshdr.md` (`## [Unreleased]` format); never edit root
   `CHANGELOG.md`.
4. `.gitignore` whitelists the seven standard folders under its Upstream `/docs/*` rule;
   `docs/tmp/*-scratch/` stays ignored.

## Alternatives rejected

- **Move Upstream docs elsewhere**: breaks Upstream scripts and conflicts on every sync.
- **Write sshdr entries into root `CHANGELOG.md`**: conflicts on every Upstream release.
- **Put the sshdr changelog under `docs/reference/changelog/`**: hides it and diverges further from
  the global changelog rule.
