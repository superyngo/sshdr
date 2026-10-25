# 0006 — TUI Desktop as a separate Client

Date: 2026-10-24
Amended by: [ADR 0007](0007-desktop-tasks-are-workspaces.md) (decisions 2 and 4)
Assessment: [`../audit/2026-10-24-desktop-env-assessment.md`](../audit/2026-10-24-desktop-env-assessment.md)

## Context

The owner defers sshi integration (**Fleet Operations**, **Remote Backup**) and turns sshdr into a
TUI **Desktop**: **Start Menu** and **Taskbar**, overlapping **Windows** with **Snap**, a
**Machines Folder** as the entry to every **Machine**, a file manager with multi-select,
drag-and-drop, and cut/copy/paste, a **File Clipboard**, **File Associations**, and later features
delivered as **Apps**.

herdr's shell **Client** receives one server-rendered **Pane Surface** for the active tab of the
selected **Endpoint**; the **Server** decides pane geometry and PTY sizes from its split tree. It
cannot show overlapping panes with independent sizes, several tabs at once, or several
**Machines** side by side. **Upstream** changed `src/client` 144 times and `src/server` 99 times in
the 90 days before this decision.

## Decision

1. **Separate Client.** The **Desktop** is a new **Client** started with `herdr desk`, beside the
   unchanged default `herdr` shell. **Upstream** files get only the CLI dispatch hook.
2. **Terminal Windows are Server panes.** Each **Terminal Window** is a pane in a dedicated
   workspace on the local **Server**, shown through one `terminal session control` stream per
   **Window** (`ClientMessage::ControlTerminal`). The **Desktop** sizes each stream to its
   **Window**, parses the ANSI output into cells, and composes the screen. Panes survive a
   **Desktop** detach. **Window** geometry, z-order, and other presentation state stay in the
   **Client**.
3. **Two kinds of Window.** **Native Windows** (**Machines Folder**, file manager, settings) are
   drawn by the **Desktop** and take part in drag-and-drop and the **File Clipboard**.
   **Terminal Windows** run PTY programs and do not.
4. **Machines.** The **Machines Folder** reads the existing endpoint catalog. A **Server Tier**
   **Machine** opens a **Terminal Window** running a nested herdr **Client**; an **SSH Tier**
   **Machine** opens `ssh`.
5. **Remote files in the first release.** The file manager works on remote **Machines** through a
   new SSH file layer over the managed OpenSSH configuration (ADR 0004). This layer is a subset of
   sshi `cp`.
6. **Apps** extend the herdr plugin manifest with **Desktop** fields (icon, **Start Menu** entry,
   **File Associations**, a window placement) instead of a separate format.
7. **Platforms.** The first release targets macOS, Linux, and Windows; Windows is dropped from it
   if the P0 spike fails there. Touch platforms (tap = press + release; no drag, right-click, or
   long-press) get a separate touch layout in a later phase.
8. The hard-fork point (ADR 0005, backlog B3) is not tied to this decision.

## Alternatives rejected

- **Desktop mode inside the existing shell Client** (float layout in the **Server** tab, new shell
  overlays): no side-by-side **Machines**, and repeated conflicts in the most-changed **Upstream**
  files.
- **Desktop as a herdr plugin**: a plugin draws only inside its own pane and cannot embed other
  panes.
- **Desktop replaces the default `herdr` entry**: deferred; both coexist.
