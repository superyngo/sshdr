//! Server state for the desk (ADR 0007): every workspace is a **Task** and every tab a
//! **Terminal Window** showing the tab's root pane. Reads state over the public API socket
//! (`session.snapshot`, `events.subscribe`); changes it with public CLI commands.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::client::{ApiClient, ConnectionTarget};
use crate::api::schema::events::{EventsSubscribeParams, Subscription};
use crate::api::schema::server::PingParams;
use crate::api::schema::{common::EmptyParams, Method, Request};

use super::stream::DeskEvent;
use super::transport::{self, ApiStream, Link, Target};

/// A window created this recently survives a snapshot taken before it existed.
pub(super) const CREATE_GRACE: Duration = Duration::from_secs(2);
/// Events arriving within this time share one snapshot.
const COALESCE: Duration = Duration::from_millis(100);
/// Wait before a failed **Target** is tried again.
const RETRY: Duration = Duration::from_secs(2);
/// A **Machine** silent this long is pinged; a ping longer than [`PING_TIMEOUT`] means
/// offline, so a stall shows within about 7 s.
const PING_EVERY: Duration = Duration::from_secs(3);
const PING_TIMEOUT: Duration = Duration::from_secs(3);
/// One call longer than this marks the **Target** offline (B22 results §4.5).
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

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

/// Path of the API socket of `target`. A **Machine** reports it (`status server --json`); a
/// stopped remote **Server** counts as **SSH Tier**, like a missing `herdr`.
fn socket_path(target: &Target, timeout: Duration) -> Result<String, CallError> {
    if let Target::Local = target {
        return Ok(ApiClient::local().socket_path().display().to_string());
    }
    let status = call(target, &["status", "server", "--json"], timeout)?;
    match (
        status.get("running").and_then(Value::as_bool),
        text(&status, "socket"),
    ) {
        (Some(true), Some(socket)) => Ok(socket),
        _ => Err(CallError {
            link: Link::Ssh,
            message: "remote herdr server is not running".into(),
        }),
    }
}

fn request_line(method: Method) -> Vec<u8> {
    let request = Request {
        id: "desk".into(),
        method,
    };
    let mut line = serde_json::to_vec(&request).unwrap_or_default();
    line.push(b'\n');
    line
}

/// One API request over the socket: in process for Local, `ssh -W` for a **Machine**.
fn api_call(
    target: &Target,
    socket: &str,
    method: Method,
    timeout: Duration,
) -> Result<Value, CallError> {
    let fail = |link, message: String| CallError { link, message };
    let value = match target {
        Target::Local => {
            let request = Request {
                id: "desk".into(),
                method,
            };
            ApiClient::for_target(ConnectionTarget::SocketPath(socket.into()))
                .request_value_with_timeout(&request, timeout)
                .map_err(|err| fail(Link::Offline, format!("api: {err}")))?
        }
        Target::Machine(machine) => {
            let command = transport::socket_command(machine, socket)
                .map_err(|err| fail(Link::Offline, format!("api: {err}")))?;
            let output = transport::exchange(command, Some(&request_line(method)), timeout)
                .map_err(|err| fail(Link::Offline, format!("api: {err}")))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let Some(line) = stdout.lines().next().filter(|l| !l.trim().is_empty()) else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let link = transport::failure_link(output.status.code(), &stderr);
                // ssh worked but nothing answered on the socket: the **Server** is gone.
                let link = if link == Link::Ssh {
                    Link::Offline
                } else {
                    link
                };
                return Err(fail(link, format!("api: {}", stderr.trim())));
            };
            serde_json::from_str(line).map_err(|err| fail(Link::Offline, format!("api: {err}")))?
        }
    };
    if let Some(error) = value.get("error") {
        return Err(fail(Link::Offline, format!("api: {error}")));
    }
    Ok(value)
}

fn snapshot_at(target: &Target, socket: &str, timeout: Duration) -> Result<Snapshot, CallError> {
    let value = api_call(
        target,
        socket,
        Method::SessionSnapshot(EmptyParams::default()),
        timeout,
    )?;
    let snap = value
        .get("result")
        .and_then(|r| r.get("snapshot"))
        .unwrap_or(&Value::Null);
    let part = |key| {
        snap.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    Ok(build_snapshot(
        &part("workspaces"),
        &part("tabs"),
        &part("panes"),
    ))
}

pub(super) fn snapshot(target: &Target, timeout: Duration) -> Result<Snapshot, CallError> {
    snapshot_at(target, &socket_path(target, timeout)?, timeout)
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

/// What wakes a [`spawn_watcher`] thread.
pub(super) enum Wake {
    /// A server event, or the desk asking for a fresh snapshot.
    Refresh,
    /// The event stream of this generation ended.
    Closed(u64),
}

/// Server events that change desk state: **Tasks**, windows, and root panes.
fn subscriptions() -> Vec<Subscription> {
    vec![
        Subscription::WorkspaceCreated {},
        Subscription::WorkspaceRenamed {},
        Subscription::WorkspaceClosed {},
        Subscription::TabCreated {},
        Subscription::TabClosed {},
        Subscription::TabRenamed {},
        Subscription::TabMoved {},
        Subscription::PaneCreated {},
        Subscription::PaneClosed {},
    ]
}

/// Follows one **Target** (B23): subscribes to server events and sends a fresh snapshot after
/// each burst, instead of polling. A **Machine** is pinged after [`PING_EVERY`] without
/// traffic so a stalled link shows `! offline`. The first attempt reports
/// [`DeskEvent::Connected`] and ends the thread on failure (**Tier** detection, ADR 0004);
/// later failures, and every failure without `report_connect`, report the [`Link`] and retry. `wake` also takes [`Wake::Refresh`] from
/// the desk, for windows whose stream ended.
pub(super) fn spawn_watcher(
    target: Target,
    report_connect: bool,
    stop: Arc<AtomicBool>,
    wake: (Sender<Wake>, Receiver<Wake>),
    events: Sender<DeskEvent>,
) {
    std::thread::spawn(move || {
        let key = target.key().to_string();
        let (tx, rx) = wake;
        let mut first = report_connect;
        let mut generation = 0;
        while !stop.load(Ordering::Relaxed) {
            generation += 1;
            let result = watch(
                &target, &key, &stop, &tx, &rx, generation, &events, &mut first,
            );
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if let Err(err) = result {
                tracing::warn!(err = %err.message, "desk: watch failed");
                let event = if first {
                    DeskEvent::Connected(key.clone(), Err(err.link))
                } else {
                    DeskEvent::Link(key.clone(), err.link)
                };
                if events.send(event).is_err() || first {
                    return;
                }
            }
            first = false;
            let until = Instant::now() + RETRY;
            while Instant::now() < until && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)] // One watch attempt; the thread state is passed in.
fn watch(
    target: &Target,
    key: &str,
    stop: &AtomicBool,
    tx: &Sender<Wake>,
    rx: &Receiver<Wake>,
    generation: u64,
    events: &Sender<DeskEvent>,
    first: &mut bool,
) -> Result<(), CallError> {
    let timeout = if *first {
        CONNECT_TIMEOUT
    } else {
        CALL_TIMEOUT
    };
    let offline = |message: String| CallError {
        link: Link::Offline,
        message,
    };
    let socket = socket_path(target, timeout)?;
    let mut stream = ApiStream::open(
        target,
        &socket,
        &request_line(Method::EventsSubscribe(EventsSubscribeParams {
            subscriptions: subscriptions(),
        })),
    )
    .map_err(|err| offline(format!("subscribe: {err}")))?;
    let Some(mut lines) = stream.lines.take() else {
        return Err(offline("subscribe: no stream".into()));
    };
    let reader_tx = tx.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        let mut started = false;
        loop {
            line.clear();
            match lines.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if !started => {
                    started = line.contains("subscription_started");
                    if !started {
                        tracing::warn!(line = %line.trim(), "desk: subscribe refused");
                        break;
                    }
                }
                Ok(_) => {
                    if reader_tx.send(Wake::Refresh).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = reader_tx.send(Wake::Closed(generation));
    });
    // Subscribe first, then snapshot: no change falls between the two.
    let snap = snapshot_at(target, &socket, timeout)?;
    let event = if *first {
        DeskEvent::Connected(key.to_string(), Ok(snap))
    } else {
        DeskEvent::Snapshot(key.to_string(), snap)
    };
    // Connected: later failures retry instead of ending the watcher.
    *first = false;
    if events.send(event).is_err() {
        return Ok(());
    }
    let mut quiet_since = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Wake::Refresh) => {
                // One snapshot per burst of events.
                std::thread::sleep(COALESCE);
                let mut closed = false;
                while let Ok(wake) = rx.try_recv() {
                    closed |= matches!(wake, Wake::Closed(g) if g == generation);
                }
                let snap = snapshot_at(target, &socket, CALL_TIMEOUT)?;
                if events
                    .send(DeskEvent::Snapshot(key.to_string(), snap))
                    .is_err()
                {
                    return Ok(());
                }
                if closed {
                    return Err(offline("event stream closed".into()));
                }
                quiet_since = Instant::now();
            }
            Ok(Wake::Closed(g)) if g == generation => {
                return Err(offline("event stream closed".into()));
            }
            Ok(Wake::Closed(_)) => {}
            Err(RecvTimeoutError::Timeout) => {
                if matches!(target, Target::Machine(_)) && quiet_since.elapsed() >= PING_EVERY {
                    api_call(
                        target,
                        &socket,
                        Method::Ping(PingParams::default()),
                        PING_TIMEOUT,
                    )?;
                    quiet_since = Instant::now();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
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

    #[test]
    fn subscribe_request_is_one_json_line_with_desk_events() {
        let line = request_line(Method::EventsSubscribe(EventsSubscribeParams {
            subscriptions: subscriptions(),
        }));
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&line).expect("json");
        assert_eq!(value["method"], "events.subscribe");
        let types: Vec<&str> = value["params"]["subscriptions"]
            .as_array()
            .expect("subscriptions")
            .iter()
            .filter_map(|s| s["type"].as_str())
            .collect();
        assert!(types.contains(&"tab.created") && types.contains(&"workspace.closed"));
    }
}
