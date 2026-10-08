# 0004 — Fleet: registered Machines with connect-time Tiers

Date: 2026-10-08
Supersedes: [0003](0003-fleet-hosts-are-machines.md)

## Context

ADR 0003 required herdr on every **Host**. That excludes hosts that cannot or should not run it
(appliances, minimal containers, unsupported platforms), which is where sshi is useful. The herdr
**Client** already detects a missing or incompatible remote **Server** on connect; today it marks the
**Machine** **Attention** (`remote/saved.rs::saved_ssh_failure_needs_attention`). The owner also
plans an sshdr **Client** panel to monitor and control connections.

## Decision

1. **Registration only.** **Hosts** enter the **Fleet** through `machine add`; no auto-discovery.
   Every registered **Machine** is a **Host**.
2. **Tier detected on every connection**, never stored in the profile:
   - **Attention**: SSH authentication or host-key failure (unchanged).
   - **SSH Tier**: SSH works, but no compatible **Server** on the host. Only **Fleet Operations**
     are available.
   - **Server Tier**: a compatible **Server** is reachable. **Fleet Operations** plus sessions,
     agents, and panes.
   An incompatible **Server** drops to **SSH Tier** with an upgrade hint instead of **Attention**.
3. **One SSH implementation of Fleet Operations for every Tier**, over the managed OpenSSH
   configuration and ControlMaster already used for **Machines**. **Server Tier** does not get a
   second, API-based implementation.
4. **Connect on demand.** **SSH Tier** hosts hold no persistent connection; an operation or an
   explicit user action opens it. Connection state is shown and controlled from a **Client** panel.
5. **Fleet data in an sshdr-owned file** keyed by profile id (like
   `~/.local/share/herdr/client/ssh-metadata/<profile-id>.json`), not in `endpoints.json`, whose
   schema is `deny_unknown_fields` and versioned.

## Alternatives rejected

- **Every Host runs a Server** (ADR 0003): excludes hosts that cannot run it.
- **Separate Host and Machine records**: two catalogs and two connection models for one target.
- **Fleet Operations via Server API on Server Tier and SSH on SSH Tier**: two implementations of
  every operation.
- **Persistent connections to every Host**: does not scale to large fleets; SSH Tier has no
  **Bridge** heartbeat to ride on.
- **Auto-discovery from `~/.ssh/config`**: host identity and trust need an explicit registration
  step.
