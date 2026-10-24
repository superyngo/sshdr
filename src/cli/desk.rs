//! sshdr desk client (`herdr desk`): overlapping terminal windows, each one a
//! `terminal session control` stream to a pane in the local server's `desk` workspace.
//! B13 spike for ADR 0006 (`docs/adr/0006-tui-desktop-as-separate-client.md`).

mod app;
mod session;
mod wm;

const USAGE: &str = "usage: herdr desk\n\n\
Open the sshdr desktop: terminal windows over panes in the `desk` workspace.\n\
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

/// Console input mode for raw VT bytes. Spike debt: moves to `src/platform/` in B14.
mod platform_input {
    #[cfg(windows)]
    pub(super) fn enable_vt_input() {
        use windows_sys::Win32::System::Console::{
            GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_INPUT,
            STD_INPUT_HANDLE,
        };
        // SAFETY: plain console API calls on the process's own stdin handle.
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT);
            }
        }
    }

    #[cfg(not(windows))]
    pub(super) fn enable_vt_input() {}
}
