//! Server side of a desk window: the pane lives in the local server's `desk` workspace and is
//! driven through a `herdr terminal session control` child process (JSON lines). Only public
//! CLI surfaces are used, so this module stays out of upstream internals.

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;

use base64::Engine;
use serde_json::{json, Value};

use super::wm::WinId;

pub(super) const DESK_WORKSPACE_LABEL: &str = "desk";

pub(super) enum DeskEvent {
    Input(Vec<u8>),
    Frame {
        win: WinId,
        width: u16,
        height: u16,
        bytes: Vec<u8>,
    },
    Closed(WinId),
}

/// A pane in the desk workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeskPane {
    pub pane_id: String,
    pub terminal_id: String,
    pub title: String,
}

fn herdr_json(args: &[&str]) -> io::Result<Value> {
    let exe = std::env::current_exe()?;
    let output = Command::new(exe).args(args).stdin(Stdio::null()).output()?;
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

fn result(value: &Value) -> &Value {
    value.get("result").unwrap_or(value)
}

fn pane_from(info: &Value) -> Option<DeskPane> {
    let pane_id = info.get("pane_id")?.as_str()?.to_string();
    let terminal_id = info.get("terminal_id")?.as_str()?.to_string();
    let title = info
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(|cwd| std::path::Path::new(cwd).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| pane_id.clone());
    Some(DeskPane {
        pane_id,
        terminal_id,
        title,
    })
}

/// Find or create the `desk` workspace; returns its id and the panes already in it.
pub(super) fn ensure_workspace() -> io::Result<(String, Vec<DeskPane>)> {
    let list = herdr_json(&["workspace", "list"])?;
    let existing = result(&list)
        .get("workspaces")
        .and_then(Value::as_array)
        .and_then(|workspaces| {
            workspaces
                .iter()
                .find(|w| w.get("label").and_then(Value::as_str) == Some(DESK_WORKSPACE_LABEL))
        })
        .and_then(|w| w.get("workspace_id")?.as_str().map(str::to_string));
    if let Some(workspace_id) = existing {
        let panes = herdr_json(&["pane", "list", "--workspace", &workspace_id])?;
        let panes = result(&panes)
            .get("panes")
            .and_then(Value::as_array)
            .map(|panes| panes.iter().filter_map(pane_from).collect())
            .unwrap_or_default();
        return Ok((workspace_id, panes));
    }
    let created = herdr_json(&[
        "workspace",
        "create",
        "--label",
        DESK_WORKSPACE_LABEL,
        "--no-focus",
    ])?;
    let created = result(&created);
    let workspace_id = created
        .pointer("/workspace/workspace_id")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("workspace create: missing workspace_id"))?
        .to_string();
    let panes = created
        .get("root_pane")
        .and_then(pane_from)
        .into_iter()
        .collect();
    Ok((workspace_id, panes))
}

pub(super) fn create_pane(workspace_id: &str) -> io::Result<DeskPane> {
    let created = herdr_json(&["tab", "create", "--workspace", workspace_id, "--no-focus"])?;
    result(&created)
        .get("root_pane")
        .and_then(pane_from)
        .ok_or_else(|| io::Error::other("tab create: missing root_pane"))
}

pub(super) fn close_pane(pane_id: &str) {
    if let Err(err) = herdr_json(&["pane", "close", pane_id]) {
        tracing::warn!(%err, pane_id, "desk: pane close failed");
    }
}

/// One `terminal session control` child streaming one pane.
pub(super) struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Session {
    pub fn spawn(
        win: WinId,
        terminal_id: &str,
        cols: u16,
        rows: u16,
        events: Sender<DeskEvent>,
    ) -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .args([
                "terminal",
                "session",
                "control",
                terminal_id,
                "--takeover",
                "--cols",
                &cols.max(1).to_string(),
                "--rows",
                &rows.max(1).to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("session control: no stdout"))?;
        std::thread::spawn(move || read_frames(win, stdout, events));
        Ok(Self { child, stdin })
    }

    fn send(&mut self, command: Value) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let mut line = command.to_string();
        line.push('\n');
        if stdin.write_all(line.as_bytes()).is_err() {
            self.stdin = None;
        }
    }

    pub fn input(&mut self, bytes: &[u8]) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.send(json!({"type": "terminal.input", "bytes": encoded}));
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols > 0 && rows > 0 {
            self.send(json!({"type": "terminal.resize", "cols": cols, "rows": rows}));
        }
    }

    pub fn mouse(&mut self, action: &str, button: &str, column: u16, row: u16, modifiers: u8) {
        self.send(json!({
            "type": "terminal.mouse",
            "action": action,
            "button": button,
            "column": column,
            "row": row,
            "modifiers": modifiers,
        }));
    }

    pub fn scroll(&mut self, up: bool, column: u16, row: u16) {
        self.send(json!({
            "type": "terminal.scroll",
            "direction": if up { "up" } else { "down" },
            "lines": 3,
            "column": column,
            "row": row,
        }));
    }

    /// Release control and let the child exit; the pane stays on the server.
    pub fn release(mut self) {
        self.send(json!({"type": "terminal.release"}));
        self.stdin = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_frames(win: WinId, stdout: std::process::ChildStdout, events: Sender<DeskEvent>) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("terminal.frame") => {
                let bytes = value
                    .get("bytes")
                    .and_then(Value::as_str)
                    .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                    .unwrap_or_default();
                let dim = |key| {
                    value
                        .get(key)
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok())
                        .unwrap_or(0)
                };
                let frame = DeskEvent::Frame {
                    win,
                    width: dim("width"),
                    height: dim("height"),
                    bytes,
                };
                if events.send(frame).is_err() {
                    return;
                }
            }
            Some("terminal.closed") => break,
            _ => {}
        }
    }
    let _ = events.send(DeskEvent::Closed(win));
}
