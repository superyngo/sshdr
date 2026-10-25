//! sshdr desk client (`herdr desk`): overlapping terminal windows, each one a
//! `terminal session control` stream to a pane in the local server's `desk` workspace.
//! ADR 0006/0007 (`docs/adr/`); plan `docs/plan/2026-10-25-b14-desk-shell.md`.

mod app;
mod dialog;
mod menu;
mod server;
mod state;
mod stream;
mod taskbar;
mod transport;
mod wm;

const USAGE: &str = "usage: herdr desk [--ssh <target> --remote-session <name>]\n\n\
Open the sshdr desktop: every workspace is a Task, every tab a window (its root pane).\n\
Ctrl+\\ then: n new · w close · m maximize · t tile · Tab next · q quit (panes stay).";

pub(super) fn run_desk_command(args: &[String]) -> std::io::Result<i32> {
    match args.first().map(String::as_str) {
        None => app::run(),
        // B22 spike: windows from `herdr` on an SSH target.
        Some("--ssh") => {
            let (Some(target), rest) = (args.get(1), &args[2.min(args.len())..]) else {
                eprintln!("{USAGE}");
                return Ok(2);
            };
            // Not `--session`: the global flag is stripped from argv before dispatch
            // (`session.rs`), which silently fell through to the remote default Session.
            // Required in the spike so a run never touches a Machine's default Session.
            let session = match rest {
                [flag, name] if flag == "--remote-session" => Some(name.clone()),
                _ => {
                    eprintln!("{USAGE}");
                    return Ok(2);
                }
            };
            transport::set_remote(Some(transport::Remote {
                target: target.clone(),
                session,
            }));
            app::run()
        }
        Some("help" | "--help" | "-h") => {
            eprintln!("{USAGE}");
            Ok(0)
        }
        Some(_) => {
            eprintln!("{USAGE}");
            Ok(2)
        }
    }
}

fn text_width(text: &str) -> u16 {
    use unicode_width::UnicodeWidthStr;
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}
