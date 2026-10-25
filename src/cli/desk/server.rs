//! Server state for the desk (ADR 0007): every workspace is a **Task** and every tab a
//! **Terminal Window** showing the tab's root pane. Uses public CLI commands only.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::stream::DeskEvent;
use super::transport::{self, Link, Target};

pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// One CLI call longer than this marks the **Target** offline (B22 results §4.5).
pub(super) const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The first call to a **Machine** may open its ControlMaster (`ConnectTimeout=10`).
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);

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

/// A failed call: the [`Link`] it implies and the error text.
#[derive(Debug)]
pub(super) struct CallError {
    pub link: Link,
    pub message: String,
}

impl From<CallError> for io::Error {
    fn from(err: CallError) -> Self {
        io::Error::other(err.message)
    }
}

fn call(target: &Target, args: &[&str], timeout: Duration) -> Result<Value, CallError> {
    let fail = |link, message| CallError { link, message };
    let command = transport::herdr(target, args, 0)
        .map_err(|err| fail(Link::Offline, format!("herdr {}: {err}", args.join(" "))))?;
    let output = transport::output_within(command, timeout)
        .map_err(|err| fail(Link::Offline, format!("herdr {}: {err}", args.join(" "))))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let link = match target {
            Target::Local => Link::Offline,
            Target::Machine(_) => transport::failure_link(output.status.code(), &stderr),
        };
        return Err(fail(
            link,
            format!("herdr {}: {}", args.join(" "), stderr.trim()),
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| fail(Link::Ssh, format!("herdr {}: {err}", args.join(" "))))
}

fn herdr_json(target: &Target, args: &[&str]) -> io::Result<Value> {
    Ok(call(target, args, CALL_TIMEOUT)?)
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

pub(super) fn snapshot(target: &Target, timeout: Duration) -> Result<Snapshot, CallError> {
    let workspaces = list(
        &call(target, &["workspace", "list"], timeout)?,
        "workspaces",
    );
    let tabs = list(&call(target, &["tab", "list"], CALL_TIMEOUT)?, "tabs");
    let panes = list(&call(target, &["pane", "list"], CALL_TIMEOUT)?, "panes");
    Ok(build_snapshot(&workspaces, &tabs, &panes))
}

/// **Tier** detection for a **Machine** (ADR 0004): a snapshot means **Server Tier**.
pub(super) fn connect(target: &Target) -> Result<Snapshot, CallError> {
    snapshot(target, CONNECT_TIMEOUT)
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

/// Background poller for one **Target**: sends a snapshot, or the [`Link`] of a failed poll,
/// every [`POLL_INTERVAL`] until `stop` is set or the desk exits.
pub(super) fn spawn_poller(target: Target, stop: Arc<AtomicBool>, events: Sender<DeskEvent>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let key = target.key().to_string();
        let event = match snapshot(&target, CALL_TIMEOUT) {
            Ok(snap) => DeskEvent::Snapshot(key, snap),
            Err(err) => {
                tracing::warn!(err = %err.message, "desk: poll failed");
                DeskEvent::Link(key, err.link)
            }
        };
        if stop.load(Ordering::Relaxed) || events.send(event).is_err() {
            return;
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
pub(super) fn create_task(
    target: &Target,
    label: Option<&str>,
) -> io::Result<(TaskInfo, WindowInfo)> {
    let mut args = vec!["workspace", "create", "--no-focus"];
    if let Some(label) = label {
        args.extend(["--label", label]);
    }
    let value = herdr_json(target, &args)?;
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

pub(super) fn create_window(target: &Target, workspace_id: &str) -> io::Result<WindowInfo> {
    created_window(&herdr_json(
        target,
        &["tab", "create", "--workspace", workspace_id, "--no-focus"],
    )?)
}

pub(super) fn close_window(target: &Target, tab_id: &str) {
    run(target, &["tab", "close", tab_id]);
}

/// Run `command` in a pane (the **SSH Tier** window runs `ssh`).
pub(super) fn run_in_pane(target: &Target, pane_id: &str, command: &str) {
    run(target, &["pane", "run", pane_id, command]);
}

fn run(target: &Target, args: &[&str]) {
    if let Err(err) = herdr_json(target, args) {
        tracing::warn!(%err, "desk: command failed");
    }
}

pub(super) fn rename_window(target: &Target, tab_id: &str, label: &str) {
    run(target, &["tab", "rename", tab_id, label]);
}

pub(super) fn rename_task(target: &Target, workspace_id: &str, label: &str) {
    run(target, &["workspace", "rename", workspace_id, label]);
}

pub(super) fn close_task(target: &Target, workspace_id: &str) {
    run(target, &["workspace", "close", workspace_id]);
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
