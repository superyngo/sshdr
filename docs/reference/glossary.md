# Glossary

Canonical vocabulary for sshdr. Code identifiers, UI strings, commit messages, and every other
document use these terms. Introducing a new term means adding its entry here in the same commit.
Terms for the planned **Fleet** and **Remote Backup** subsystems are fixed here before any code is
written; their behavior is specified later under [`../spec/`](../spec/README.md).

## Fork

**sshdr**:
This repository: a fork of **Upstream** that adds **Fleet** management and **Remote Backup** on top
of the herdr terminal agent runtime. The binary name is still `herdr`; renaming is an open decision
(see [`../plan/BACKLOG.md`](../plan/BACKLOG.md)).
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
A saved SSH target paired with a herdr **Session** running on that remote host, used by the
**Client** to attach to a remote **Server**. Requires herdr installed remotely. Not a **Host**.
_Avoid_: remote, server (for this concept), host.

**Endpoint**:
The identity of a connection source for a **Client**: local, or one **Machine**.
_Avoid_: target (for this concept).

## Fleet

**Fleet**:
The set of remote **Hosts** sshdr manages directly over SSH, without requiring herdr on them.
_Avoid_: cluster, farm, inventory.

**Host**:
One member of the **Fleet**, addressed by an SSH config alias, with a detected shell type and zero
or more **Groups**. A host may also be a **Machine**, but the two are separate records.
_Avoid_: node, server, machine.

**Group**:
A named label on **Hosts** used for target selection.
_Avoid_: tag, role.

**Fleet Operation**:
An action fanned out to selected **Hosts** in parallel: check, run, exec, copy, or sync.
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
