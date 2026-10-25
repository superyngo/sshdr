# 0007 — Desktop Tasks are workspaces; remote tabs are Windows

Date: 2026-10-25
Amends: [ADR 0006](0006-tui-desktop-as-separate-client.md) decisions 2 and 4
Evidence: [B13 spike results](../audit/2026-10-24-b13-desk-spike.md); backlog B20

## Context

After the B13 spike the owner asked for a minimize button that groups **Windows** under **Taskbar**
buttons, right-click rename for **Windows** and groups, and a **Desktop** hierarchy of
**Machine** → **Task** → **Window**. herdr's hierarchy is **Machine** → **Server** → **Session** →
workspace → tab → pane. The spike put every **Window** in one dedicated `desk` workspace, attached
every pane in it (including the `Sidebar` pane herdr-sidebar adds to each new workspace), and
ADR 0006 showed a **Server Tier** **Machine** as one **Window** running a nested herdr **Client**,
so remote tabs could not be minimized, grouped, or renamed on the **Desktop**.

## Decision

1. **Task = workspace.** Every workspace of the attached **Session** is a **Task**, shown as one
   **Taskbar** button; the **Desktop** no longer uses only a dedicated `desk` workspace. Renaming a
   **Task** calls `workspace rename` on its **Server**.
2. **Terminal Window = tab.** Each tab is one **Terminal Window** showing the tab's root pane
   through one `terminal session control` stream. Other panes split into that tab are not shown;
   the **Desktop** never splits. Renaming a **Window** calls `tab rename`.
3. **Remote Machines are decomposed.** For a **Server Tier** **Machine**, the **Desktop** lists that
   **Server**'s workspaces and tabs and opens one stream per remote tab by running the same CLI
   commands on the **Machine** over SSH with the managed OpenSSH configuration (ADR 0004). Remote
   **Tasks** and **Windows** behave like local ones. An **SSH Tier** **Machine** still opens one
   **Terminal Window** running `ssh`.
4. **Minimize** hides a **Window** into its **Task**'s **Taskbar** button; clicking the button lists
   the **Task**'s **Windows**. Minimized state and geometry are **Client** presentation state.

## Consequences

- The `Sidebar`-pane problem from B13 disappears: plugin panes split into a tab are not root panes.
- Remote streams over SSH are unverified (latency, disconnects, **Attention**, sharing one
  ControlMaster across many streams); backlog B22 verifies them before B15.
- The **Desktop** shows the workspaces of the default herdr shell **Client** too; both edit the same
  names.

## Alternatives rejected

- **Task = Session**: each **Task** would need its own **Server** instance; workspaces are herdr's
  existing grouping.
- **Nested Client per Machine** (ADR 0006 decision 4): remote tabs could not be minimized, grouped,
  renamed, or snapped next to local ones.
