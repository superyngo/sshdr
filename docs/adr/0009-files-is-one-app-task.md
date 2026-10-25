# 0009 — Files is one App; its windows share one Task

Date: 2026-10-25
Amends: [ADR 0008](0008-desktop-windows-are-workspaces.md) decision 1 (closes B27)
Evidence: spec [Desk Files](../spec/2026-10-25-desk-files.md)

## Context

ADR 0008 made each **Task** a **Machine** grouping its **Terminal Windows**, and left open where
**Native Windows** (the **Machines Folder**, later **Apps**) belong. The owner asked for one file
manager for Local and remote **Machines** (B16, B17) that also holds the **Machines** list, and
considered treating a **Machine**'s **Task** as one **App**.

## Decision

1. **Files** is the **Desktop**'s file manager **App**, drawn as **Native Windows**. A **Files**
   window browses one location at a time: the `Machines` root (the former **Machines Folder**) or
   a folder on a **Machine**.
2. Every **Files** window belongs to one **Files** **Task** on the **Taskbar**, whichever
   **Machine** it shows. **Terminal Windows** stay grouped by **Machine** (ADR 0008).
3. A **Task** therefore groups the windows of one **App**: the terminal **App** per **Machine**,
   and **Files** once. Later **Apps** get their own **Task** the same way.
4. View and sort are **Client** presentation state, saved in `desk.json` only: a default per
   location kind (`Machines` root, folders) plus each open window's own, restored with it.

## Consequences

- A **Files** window keeps its **Taskbar** button while it navigates between **Machines**.
- The **Machines Folder** becomes the `Machines` root of **Files**; the Start Menu's Machines
  entry opens it there.
- Disconnecting a **Machine** does not close **Files** windows; they show it as not connected.

## Alternatives rejected

- **Group Files windows under the Machine they show**: the button moves whenever the window
  crosses **Machines**.
- **Native Windows without a Task**: they could not be minimized or found from the **Taskbar**.
- **Remember view and sort per folder** (Explorer): entries pile up and go stale, and the
  behavior is hard to predict.
