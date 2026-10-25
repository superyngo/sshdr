# 0008 — Desktop Windows are workspaces; Tasks are Machines

Date: 2026-10-25
Amends: [ADR 0007](0007-desktop-tasks-are-workspaces.md) decisions 1, 2, and 4
Evidence: plan [B24–B26](../plan/2026-10-25-b24-b25-desk-hierarchy.md)

## Context

ADR 0007 made every workspace a **Task** and every tab a **Terminal Window**. The **Machine**
then showed only as a `<machine>:` prefix on **Taskbar** buttons and titles, while herdr's own
sidebar groups **Machine** → workspace → tab. The owner found the **Desktop** hierarchy did not
match herdr's and asked to realign it.

## Decision

1. **Task = Machine.** The **Taskbar** has one button per **Machine**: Local always, and each
   connected **Machine** or one with open windows. Its name is the **Machine**'s label and cannot
   be renamed on the **Desktop**. An **SSH Tier** **Machine**'s `ssh` windows are local
   workspaces that its **Task** holds; the **Desktop** records them in `desk.json`.
2. **Terminal Window = workspace.** The window's title is the workspace name (`workspace rename`),
   its first row is a tab strip (`+` creates a tab, `×` closes one, right click renames), and its
   content shows the shown tab. `[×]` closes the workspace, asking first when it has more than one
   tab.
3. **The shown tab is server state.** Choosing a tab runs `tab focus`, and the window follows the
   workspace's `active_tab_id` from any **Client**.
4. **Minimize** hides a **Window** into its **Machine**'s **Taskbar** button.

## Consequences

- Fewer, stable **Taskbar** buttons; a **Machine**'s windows are listed under its button.
- `tab focus` also makes that workspace the **Server**'s focused one, so a herdr shell **Client**
  on the same **Session** follows the **Desktop**'s choice, and the other way round.
- `desk.json` window entries are keyed by workspace; entries keyed by terminal id from earlier
  builds no longer match, so windows open stacked once.
- Where **Native Windows** (the **Machines Folder**, later **Apps**) belong is open: the owner is
  considering treating a **Machine**'s **Task** as one **App**.

## Alternatives rejected

- **Keep ADR 0007**: hides the **Machine** level that herdr's own sidebar shows.
- **Switch tabs only on the Desktop**: the owner chose one shown tab per workspace for every
  **Client**.
