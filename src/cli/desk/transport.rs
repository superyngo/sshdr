//! B22 spike: where desk runs `herdr` — this binary, or `herdr` on a **Machine** over SSH.
//! Every SSH call shares herdr's ControlMaster socket (`platform::shared_ssh_control_path`) and
//! runs with `BatchMode=yes`, so a Machine that needs a human never prompts inside the desk.

use std::io;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub(super) struct Remote {
    pub target: String,
    pub session: Option<String>,
}

static REMOTE: OnceLock<Option<Remote>> = OnceLock::new();

pub(super) fn set_remote(remote: Option<Remote>) {
    let _ = REMOTE.set(remote);
}

pub(super) fn remote() -> Option<&'static Remote> {
    REMOTE.get().and_then(Option::as_ref)
}

/// A command that runs `herdr <args>` on the desk's target.
pub(super) fn herdr(args: &[&str]) -> io::Result<Command> {
    let Some(remote) = remote() else {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(args);
        return Ok(command);
    };
    let control =
        crate::platform::shared_ssh_control_path(&crate::config::config_path(), &remote.target)?;
    let mut command = Command::new("ssh");
    command.args(["-T", "-o", "BatchMode=yes", "-o", "ControlMaster=auto"]);
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
    command.arg(&remote.target).arg("--");
    let mut line = String::new();
    if let Some(session) = &remote.session {
        line.push_str(&format!("env HERDR_SESSION={} ", quote(session)));
    }
    line.push_str("herdr");
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    command.arg(line);
    Ok(command)
}

/// **Attention** when SSH says a human must act; otherwise the link is just down.
pub(super) fn link_status(stderr: &str) -> &'static str {
    const NEEDS_HUMAN: [&str; 4] = [
        "Permission denied",
        "Host key verification failed",
        "Too many authentication failures",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
    ];
    if NEEDS_HUMAN.iter().any(|needle| stderr.contains(needle)) {
        "! auth"
    } else {
        "! offline"
    }
}

/// POSIX single-quote for the remote shell.
fn quote(arg: &str) -> String {
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
    fn auth_failures_are_attention() {
        assert_eq!(
            link_status("wen@h: Permission denied (publickey)."),
            "! auth"
        );
        assert_eq!(link_status("Host key verification failed."), "! auth");
        assert_eq!(
            link_status("ssh: connect to host h port 22: Operation timed out"),
            "! offline"
        );
    }
}
