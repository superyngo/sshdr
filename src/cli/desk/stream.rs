//! Stream of one desk window: a `herdr terminal session control` child process (JSON lines)
//! driving the window's pane. Only public CLI surfaces are used.

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::Sender;

use base64::Engine;
use serde_json::{json, Value};

use super::wm::WinId;

pub(super) enum DeskEvent {
    Input(Vec<u8>),
    Frame {
        win: WinId,
        width: u16,
        height: u16,
        bytes: Vec<u8>,
    },
    Closed(WinId),
    /// Periodic server state (`server::poll`).
    Snapshot(super::server::Snapshot),
    /// B22: result of the last poll; `Some("! auth" | "! offline")` when the target is unreachable.
    Link(Option<&'static str>),
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
        let (cols, rows) = (cols.max(1).to_string(), rows.max(1).to_string());
        let mut child = super::transport::herdr(&[
            "terminal",
            "session",
            "control",
            terminal_id,
            "--takeover",
            "--cols",
            &cols,
            "--rows",
            &rows,
        ])?
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
