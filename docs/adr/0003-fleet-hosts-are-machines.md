# 0003 — Fleet Hosts are Machines

Date: 2026-10-07

## Context

sshdr planned a **Fleet** of **Hosts** managed over plain SSH by sshi code (`russh` `SessionPool`),
without herdr on them. herdr already has a mature multi-machine model: saved **Machines** in
`EndpointCatalog`, one **Client** supervising Local plus N SSH connections with health checks and
backoff, a machine-grouped sidebar, and `herdr --machine <id> <cmd>` forwarding API requests through
`remote-api-bridge` (see [`../reference/server-client-remote.md`](../reference/server-client-remote.md)).
After testing it, the owner judged that experience good enough to build the **Fleet** on it.

## Decision

Every **Host** is a **Machine**: herdr is installed and a herdr **Server** runs on it. **Fleet
Operations** are implemented as herdr API methods executed by each host's **Server** and reached
through the existing **Bridge** path; the local side dispatches and aggregates. **Groups** label
**Machines**.

Consequence: a **Fleet** member must be a platform herdr supports as a remote server (Linux and macOS
on x86_64/aarch64, Windows on x86_64; Windows ARM64 best-effort).

## Alternatives rejected

- **Shared skeleton, herdr-less Hosts**: reuse catalog, SSH config, health, and sidebar, but run
  operations over plain SSH. Keeps two record kinds and two transports.
- **Single record with capability levels** (SSH-only vs herdr-enabled): most flexible, but changes
  Upstream's `EndpointCatalog` and raises sync cost.
