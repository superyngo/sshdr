//! Server state for the desk (ADR 0007): every workspace is a **Task** and every tab a
//! **Terminal Window** showing the tab's root pane. Uses public CLI commands only.

use std::io;
use std::process::Stdio;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use super::stream::DeskEvent;

pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaskInfo {
    pub workspace_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WindowInfo {
    pub tab_id: String,
    pub workspace_id: String,
    pub label: String,
    pub pane_id: String,
    pub terminal_id: String,
    /// Panes in the tab besides the root pane (not shown on the desk).
    pub other_panes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub tasks: Vec<TaskInfo>,
    pub windows: Vec<WindowInfo>,
}

fn herdr_json(args: &[&str]) -> io::Result<Value> {
    let output = super::transport::herdr(args)?
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "herdr {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| io::Error::other(format!("herdr {}: {err}", args.join(" "))))
}

fn list(value: &Value, key: &str) -> Vec<Value> {
    value
        .get("result")
        .unwrap_or(value)
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

pub(super) fn snapshot() -> io::Result<Snapshot> {
    let workspaces = list(&herdr_json(&["workspace", "list"])?, "workspaces");
    let tabs = list(&herdr_json(&["tab", "list"])?, "tabs");
    let panes = list(&herdr_json(&["pane", "list"])?, "panes");
    Ok(build_snapshot(&workspaces, &tabs, &panes))
}

/// Joins the three list results. A tab's window shows `root_pane_id`; servers without that
/// field fall back to the tab's first pane without a label (plugin panes carry labels).
pub(super) fn build_snapshot(workspaces: &[Value], tabs: &[Value], panes: &[Value]) -> Snapshot {
    let tasks = workspaces
        .iter()
        .filter_map(|ws| {
            Some(TaskInfo {
                workspace_id: text(ws, "workspace_id")?,
                label: text(ws, "label").unwrap_or_default(),
            })
        })
        .collect();
    let windows = tabs
        .iter()
        .filter_map(|tab| {
            let tab_id = text(tab, "tab_id")?;
            let in_tab: Vec<&Value> = panes
                .iter()
                .filter(|p| text(p, "tab_id").as_deref() == Some(&tab_id))
                .collect();
            let root = match text(tab, "root_pane_id") {
                Some(root) => in_tab
                    .iter()
                    .find(|p| text(p, "pane_id").as_deref() == Some(&root))?,
                None => in_tab
                    .iter()
                    .find(|p| p.get("label").is_none_or(Value::is_null))
                    .or(in_tab.first())?,
            };
            Some(WindowInfo {
                workspace_id: text(tab, "workspace_id")?,
                label: text(tab, "label").unwrap_or_default(),
                pane_id: text(root, "pane_id")?,
                terminal_id: text(root, "terminal_id")?,
                other_panes: in_tab.len().saturating_sub(1),
                tab_id,
            })
        })
        .collect();
    Snapshot { tasks, windows }
}

/// Background poller: sends a [`Snapshot`] every [`POLL_INTERVAL`] until the desk exits.
pub(super) fn spawn_poller(events: Sender<DeskEvent>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        let (snap, link) = match snapshot() {
            Ok(snap) => (Some(snap), None),
            Err(err) => {
                tracing::warn!(%err, "desk: poll failed");
                (None, Some(super::transport::link_status(&err.to_string())))
            }
        };
        if events.send(DeskEvent::Link(link)).is_err() {
            return;
        }
        if let Some(snap) = snap {
            if events.send(DeskEvent::Snapshot(snap)).is_err() {
                return;
            }
        }
    });
}

fn created_window(value: &Value) -> io::Result<WindowInfo> {
    let result = value.get("result").unwrap_or(value);
    let missing = || io::Error::other("create: missing tab or root_pane");
    let tab = result.get("tab").ok_or_else(missing)?;
    let root = result.get("root_pane").ok_or_else(missing)?;
    Ok(WindowInfo {
        tab_id: text(tab, "tab_id").ok_or_else(missing)?,
        workspace_id: text(tab, "workspace_id").ok_or_else(missing)?,
        label: text(tab, "label").unwrap_or_default(),
        pane_id: text(root, "pane_id").ok_or_else(missing)?,
        terminal_id: text(root, "terminal_id").ok_or_else(missing)?,
        other_panes: 0,
    })
}

/// New **Task** (workspace); returns it and its first window.
pub(super) fn create_task() -> io::Result<(TaskInfo, WindowInfo)> {
    let value = herdr_json(&["workspace", "create", "--no-focus"])?;
    let result = value.get("result").unwrap_or(&value);
    let ws = result
        .get("workspace")
        .ok_or_else(|| io::Error::other("workspace create: missing workspace"))?;
    let task = TaskInfo {
        workspace_id: text(ws, "workspace_id")
            .ok_or_else(|| io::Error::other("workspace create: missing workspace_id"))?,
        label: text(ws, "label").unwrap_or_default(),
    };
    Ok((task, created_window(&value)?))
}

pub(super) fn create_window(workspace_id: &str) -> io::Result<WindowInfo> {
    created_window(&herdr_json(&[
        "tab",
        "create",
        "--workspace",
        workspace_id,
        "--no-focus",
    ])?)
}

pub(super) fn close_window(tab_id: &str) {
    if let Err(err) = herdr_json(&["tab", "close", tab_id]) {
        tracing::warn!(%err, tab_id, "desk: tab close failed");
    }
}

fn run(args: &[&str]) {
    if let Err(err) = herdr_json(args) {
        tracing::warn!(%err, "desk: command failed");
    }
}

pub(super) fn rename_window(tab_id: &str, label: &str) {
    run(&["tab", "rename", tab_id, label]);
}

pub(super) fn rename_task(workspace_id: &str, label: &str) {
    run(&["workspace", "rename", workspace_id, label]);
}

pub(super) fn close_task(workspace_id: &str) {
    run(&["workspace", "close", workspace_id]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn windows_use_root_pane_id_over_layout_order() {
        let ws = [json!({"workspace_id": "w_1", "label": "api"})];
        let tabs = [
            json!({"tab_id": "w_1:1", "workspace_id": "w_1", "label": "1",
            "root_pane_id": "w_1-2"}),
        ];
        let panes = [
            json!({"pane_id": "w_1-1", "terminal_id": "t1", "tab_id": "w_1:1", "label": "Sidebar"}),
            json!({"pane_id": "w_1-2", "terminal_id": "t2", "tab_id": "w_1:1"}),
        ];
        let snap = build_snapshot(&ws, &tabs, &panes);
        assert_eq!(snap.tasks.len(), 1);
        assert_eq!(snap.windows[0].terminal_id, "t2");
        assert_eq!(snap.windows[0].other_panes, 1);
    }

    #[test]
    fn older_servers_fall_back_to_first_unlabelled_pane() {
        let tabs = [json!({"tab_id": "w_1:1", "workspace_id": "w_1", "label": "1"})];
        let panes = [
            json!({"pane_id": "w_1-1", "terminal_id": "t1", "tab_id": "w_1:1", "label": "Sidebar"}),
            json!({"pane_id": "w_1-2", "terminal_id": "t2", "tab_id": "w_1:1", "label": null}),
        ];
        let snap = build_snapshot(&[], &tabs, &panes);
        assert_eq!(snap.windows[0].pane_id, "w_1-2");
    }

    #[test]
    fn tab_without_panes_has_no_window() {
        let tabs = [json!({"tab_id": "w_1:1", "workspace_id": "w_1", "label": "1"})];
        assert!(build_snapshot(&[], &tabs, &[]).windows.is_empty());
    }
}
