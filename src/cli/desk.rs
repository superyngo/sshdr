//! sshdr desk client (`herdr desk`): overlapping terminal windows, each one a
//! `terminal session control` stream to a pane in the local server's `desk` workspace.
//! ADR 0006/0007 (`docs/adr/`); plan `docs/plan/2026-10-25-b14-desk-shell.md`.

mod app;
mod server;
mod stream;
mod taskbar;
mod wm;

const USAGE: &str = "usage: herdr desk\n\n\
Open the sshdr desktop: every workspace is a Task, every tab a window (its root pane).\n\
Ctrl+\\ then: n new · w close · m maximize · t tile · Tab next · q quit (panes stay).";

pub(super) fn run_desk_command(args: &[String]) -> std::io::Result<i32> {
    match args.first().map(String::as_str) {
        None => app::run(),
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
