//! Where desk runs `herdr`: this binary for the local **Server**, or `herdr` on a **Machine** over
//! SSH (ADR 0007 decision 3). SSH calls share herdr's ControlMaster socket
//! (`platform::shared_ssh_control_path`) and run with `BatchMode=yes`, so a **Machine** that
//! needs a human never prompts inside the desk (B22 results §4).

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Streams per ControlMaster. sshd's default `MaxSessions` is 10; the rest is left for polls
/// and herdr's own client on the same master (B22 results §4.4).
pub(super) const STREAMS_PER_MASTER: usize = 8;

/// A saved **Machine** (`herdr machine add`), as desk needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Machine {
    pub id: String,
    pub label: String,
    pub target: String,
    pub session: String,
}

/// Where a **Task** or **Window** lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    Local,
    Machine(Machine),
}

pub(super) const LOCAL_KEY: &str = "local";

impl Target {
    /// Stable identity: `local` or the **Machine** profile id.
    pub fn key(&self) -> &str {
        match self {
            Target::Local => LOCAL_KEY,
            Target::Machine(machine) => &machine.id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Target::Local => "Local",
            Target::Machine(machine) => &machine.label,
        }
    }
}

/// Saved **Machines**: enabled endpoint catalog profiles, then those saved with
/// `machine add --ssh-only` (B11, default Session). Read only; the **Tier** of each is still
/// detected on connect (ADR 0004).
pub(super) fn machines() -> Vec<Machine> {
    let mut out: Vec<Machine> = match crate::client::endpoint::EndpointCatalog::load_profiles() {
        Ok(profiles) => profiles
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| Machine {
                id: p.id.to_string(),
                label: p.label,
                target: p.target,
                session: p.session,
            })
            .collect(),
        Err(err) => {
            tracing::warn!(%err, "desk: reading saved machines failed");
            Vec::new()
        }
    };
    match crate::cli::ssh_machines::load() {
        Ok(machines) => out.extend(machines.into_iter().map(|m| Machine {
            id: m.id.to_string(),
            label: m.label,
            target: m.target,
            session: crate::session::DEFAULT_SESSION_NAME.to_owned(),
        })),
        Err(err) => tracing::warn!(%err, "desk: reading ssh-only machines failed"),
    }
    out
}

/// A command that runs `herdr <args>` on `target`; remote calls use ControlMaster `master`
/// (0 is the one herdr itself uses).
pub(super) fn herdr(target: &Target, args: &[&str], master: usize) -> io::Result<Command> {
    let Target::Machine(machine) = target else {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(args);
        return Ok(command);
    };
    let mut command = ssh(&machine.target, master, "auto")?;
    let mut line = format!("env HERDR_SESSION={} herdr", quote(&machine.session));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    command.arg("-T").arg(&machine.target).arg("--").arg(line);
    Ok(command)
}

/// Make sure ControlMaster `master` of `target` is up before streams use it. Streams started
/// together with `ControlMaster=auto` race: the losers keep their own TCP connections.
pub(super) fn ensure_master(target: &Target, master: usize) -> io::Result<()> {
    let Target::Machine(machine) = target else {
        return Ok(());
    };
    let mut check = ssh(&machine.target, master, "auto")?;
    check.args(["-O", "check"]).arg(&machine.target);
    if output_within(check, Duration::from_secs(2)).is_ok_and(|o| o.status.success()) {
        return Ok(());
    }
    // `-f` leaves the master running with our stdio; null keeps it from holding a pipe open.
    let mut start = ssh(&machine.target, master, "yes")?;
    let mut child = start
        .args(["-f", "-N"])
        .arg(&machine.target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("ssh master {master}: {status}")))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ssh master timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `ssh` with the shared ControlMaster options; the caller adds the target and command.
fn ssh(target: &str, master: usize, control_master: &str) -> io::Result<Command> {
    let namespace = crate::config::config_path();
    let control = match master {
        0 => crate::platform::shared_ssh_control_path(&namespace, target)?,
        n => crate::platform::shared_ssh_control_path(&namespace, &format!("{target}#desk{n}"))?,
    };
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o"]);
    command.arg(format!("ControlMaster={control_master}"));
    command
        .arg("-o")
        .arg(format!("ControlPath={}", control.display()));
    command.args([
        "-o",
        "ControlPersist=600",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=4",
        "-o",
        "ConnectTimeout=10",
    ]);
    Ok(command)
}

/// Run `command` to completion; kill it after `timeout`.
pub(super) fn output_within(command: Command, timeout: Duration) -> io::Result<Output> {
    exchange(command, None, timeout)
}

/// Like [`output_within`], writing `input` to stdin first; stdin then closes.
pub(super) fn exchange(
    mut command: Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> io::Result<Output> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(input);
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// `ssh -W <socket>`: stdio joined to the **Machine**'s API socket over ControlMaster 0, so
/// API requests reach the remote **Server** without starting a remote `herdr` process.
pub(super) fn socket_command(machine: &Machine, socket: &str) -> io::Result<Command> {
    let mut command = ssh(&machine.target, 0, "auto")?;
    command.arg("-W").arg(socket).arg(&machine.target);
    Ok(command)
}

/// A long-lived API connection (`events.subscribe`): lines from the **Server**, and the ssh
/// child for a **Machine** (killed on drop; a local stream ends with the **Server**). The
/// reader takes `lines`; dropping the stream ends a remote reader.
pub(super) struct ApiStream {
    pub lines: Option<Box<dyn BufRead + Send>>,
    child: Option<Child>,
    /// Kept open: closing stdin would end the remote stream.
    _stdin: Option<ChildStdin>,
}

impl ApiStream {
    /// Open `socket` on `target` and send `request` (one JSON line).
    pub(super) fn open(target: &Target, socket: &str, request: &[u8]) -> io::Result<Self> {
        match target {
            Target::Local => {
                let mut stream = crate::ipc::connect_local_stream(std::path::Path::new(socket))?;
                stream.write_all(request)?;
                Ok(Self {
                    lines: Some(Box::new(BufReader::new(stream))),
                    child: None,
                    _stdin: None,
                })
            }
            Target::Machine(machine) => {
                let mut child = socket_command(machine, socket)?
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?;
                let mut stdin = child.stdin.take();
                let stdout = child.stdout.take();
                let (Some(input), Some(stdout)) = (stdin.as_mut(), stdout) else {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::other("ssh stream without pipes"));
                };
                input.write_all(request)?;
                input.flush()?;
                Ok(Self {
                    lines: Some(Box::new(BufReader::new(stdout))),
                    child: Some(child),
                    _stdin: stdin,
                })
            }
        }
    }
}

impl Drop for ApiStream {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn drain<R: io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

/// Connection state of a **Target**, shown in the **Machines Folder** and the **Taskbar**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Link {
    NotConnected,
    Connecting,
    /// **Server Tier**.
    Server,
    /// **SSH Tier**: SSH works, no usable **Server**.
    Ssh,
    /// **Attention**: a human must authenticate or confirm a host key.
    Auth,
    Offline,
}

impl Link {
    pub fn text(self) -> &'static str {
        match self {
            Link::NotConnected => "not connected",
            Link::Connecting => "connecting…",
            Link::Server => "Server",
            Link::Ssh => "SSH",
            Link::Auth => "! auth",
            Link::Offline => "! offline",
        }
    }
}

/// Classify a failed remote call. ssh exits 255 for its own failures; anything else came from
/// the remote command, so SSH itself works (**SSH Tier**).
pub(super) fn failure_link(code: Option<i32>, stderr: &str) -> Link {
    const NEEDS_HUMAN: [&str; 4] = [
        "Permission denied",
        "Host key verification failed",
        "Too many authentication failures",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
    ];
    if NEEDS_HUMAN.iter().any(|needle| stderr.contains(needle)) {
        Link::Auth
    } else if code == Some(255) || code.is_none() {
        Link::Offline
    } else {
        Link::Ssh
    }
}

/// POSIX single-quote for the remote shell.
pub(super) fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=,".contains(&b))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_keeps_plain_args_and_escapes_shell_text() {
        assert_eq!(quote("w1:t2"), "w1:t2");
        assert_eq!(quote("my task"), "'my task'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn failures_split_into_attention_offline_and_ssh_tier() {
        assert_eq!(
            failure_link(Some(255), "wen@h: Permission denied (publickey)."),
            Link::Auth
        );
        assert_eq!(
            failure_link(Some(255), "Host key verification failed."),
            Link::Auth
        );
        assert_eq!(
            failure_link(
                Some(255),
                "ssh: connect to host h port 22: Operation timed out"
            ),
            Link::Offline
        );
        assert_eq!(failure_link(None, ""), Link::Offline);
        assert_eq!(
            failure_link(Some(127), "sh: herdr: command not found"),
            Link::Ssh
        );
    }

    #[test]
    fn output_within_kills_slow_commands() {
        let mut slow = Command::new("sleep");
        slow.arg("5");
        let started = Instant::now();
        let err = output_within(slow, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
