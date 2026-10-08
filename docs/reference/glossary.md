# Glossary

Canonical vocabulary for sshdr. Code identifiers, UI strings, commit messages, and every other
document use these terms. Introducing a new term means adding its entry here in the same commit.
Terms for the planned **Fleet** and **Remote Backup** subsystems are fixed here before any code is
written; their behavior is specified later under [`../spec/`](../spec/README.md).

## Fork

**sshdr**:
This repository: a fork of **Upstream** that adds **Fleet** management and **Remote Backup** on top
of the herdr terminal agent runtime. The binary name stays `herdr` until the
hard-fork rename ([ADR 0005](../adr/0005-sync-now-hard-fork-later.md)).
_Avoid_: herdr fork, the fork (in docs titles).

**Upstream**:
The original project, `herdrdev/herdr`, reachable as the git remote `upstream`.
_Avoid_: origin, parent repo.

**Mirror Branch**:
The `master` branch. It only fast-forwards to `upstream/master` and never carries sshdr commits.
_Avoid_: upstream branch, vendor branch.

**Mainline**:
The `main` branch, the default branch of sshdr. All sshdr work lands here; **Upstream** changes
enter by merging the **Mirror Branch**.
_Avoid_: dev branch, fork branch.

## Inherited from herdr

**Server**:
The herdr background process that owns sessions, PTYs, and terminal state, locally or on a remote
machine.
_Avoid_: daemon (in docs).

**Client**:
A TUI or CLI process that attaches to a **Server** over a local socket or an SSH bridge.
_Avoid_: frontend.

**Session**:
A named multiplexer instance on a **Server**, holding workspaces, tabs, and panes.
_Avoid_: instance.

**Machine**:
A saved SSH target registered with `machine add`. Its **Tier** is detected on each connection; at
**Server Tier** it is paired with a **Session** on that host. Every Machine is a **Host**.
_Avoid_: remote, server (for this concept).

**Attention**:
The connection status of a **Machine** that needs a human action (SSH authentication, host-key
approval) before it can connect; shown as `! auth` or `! error`.
_Avoid_: error state, offline.

**Endpoint**:
The identity of a connection source for a **Client**: local, or one **Machine**.
_Avoid_: target (for this concept).

**Bridge**:
The `herdr remote-client-bridge` or `herdr remote-api-bridge` process started over SSH on a
**Machine**, piping SSH stdio to that **Server**'s client or API socket.
_Avoid_: tunnel, proxy.

**Shell Snapshot**:
The `ClientShellSnapshot` a **Server** sends to a **Client**: semantic session data (workspaces,
tabs, panes, agents, keybindings) from which the **Client** draws its chrome.
_Avoid_: state dump.

**Pane Surface**:
The rendered cell grid of panes (`PaneSurfaceFrame`/`PaneSurfacePatch`) a **Server** streams to
the **Client** for the selected **Endpoint** only.
_Avoid_: screen, framebuffer.

## TUI

**Component Kit**:
The planned sshdr-owned set of reusable **Client** TUI components (list, panel, button, popup
placement, text) that new screens are built from; requirements in
[`../spec/2026-10-07-tui-component-kit.md`](../spec/2026-10-07-tui-component-kit.md).
_Avoid_: widget library, UI framework.

## Fleet

**Fleet**:
The set of **Hosts** sshdr manages, all registered with `machine add`; members need only SSH
([ADR 0004](../adr/0004-fleet-registered-machines-with-connect-time-tiers.md)).
_Avoid_: cluster, farm, inventory.

**Host**:
A **Machine** in its role as a **Fleet** member, with zero or more **Groups**. Host and Machine are
one record; "Host" is used when speaking of the **Fleet**.
_Avoid_: node, server.

**Tier**:
The capability level of a **Host**, detected on each connection and never stored: **SSH Tier**
(SSH works, no compatible **Server**; **Fleet Operations** only) or **Server Tier** (a compatible
**Server** is reachable; adds sessions, agents, and panes). Authentication or host-key failures are
**Attention**, not a Tier.
_Avoid_: level, mode, grade.

**Group**:
A named label on **Hosts** used for **Fleet Operation** target selection.
_Avoid_: tag, role.

**Fleet Operation**:
An action fanned out to selected **Hosts** in parallel (check, run, exec, copy, or sync) over SSH,
with one implementation for every **Tier**.
_Avoid_: job, task, batch.

## Remote Backup

**Remote Backup**:
The subsystem that creates, prunes, and restores **Archives** of paths on **Fleet** **Hosts**.
_Avoid_: remote bkp, cluster backup.

**Backup Profile**:
A named set of backup settings: sources, target **Hosts**, archive location, and **Retention
Policy**.
_Avoid_: backup config, preset, job.

**Archive**:
A `.tar.gz` file produced by one backup run of a **Backup Profile** on one **Host**.
_Avoid_: backup (as a noun), tarball, snapshot.

**Prune**:
Removing **Archives** that exceed the **Retention Policy**.
_Avoid_: rotate, cleanup.

**Restore**:
Extracting an **Archive** back to its original paths or to a chosen directory.
_Avoid_: recover, unpack.

**Retention Policy**:
The rules (age, minimum kept, maximum kept) that decide which **Archives** **Prune** removes.
_Avoid_: rotation policy.
