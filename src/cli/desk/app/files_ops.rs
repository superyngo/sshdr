//! app.rs glue for **Files** operations (F3b): the **File Clipboard**, dialogs before an
//! operation, running it off the UI thread, undo, and Copy path (OSC 52).

use std::io::Write;

use base64::Engine;

use super::{App, DialogTarget, Target, LOCAL_KEY};
use crate::cli::desk::dialog::Dialog;
use crate::cli::desk::files::ops::{self, Done, Op};
use crate::cli::desk::files::remote;
use crate::cli::desk::files::{Ask, Clip, Request};
use crate::cli::desk::wm::WinId;

impl App {
    /// Requests of `files` that operate on files; the others return unhandled.
    pub(super) fn files_op_request(&mut self, win: WinId, request: Request) {
        let machine = self
            .files
            .get(&win)
            .and_then(|f| f.folder())
            .map(|(m, _)| m.to_string());
        let shared = &mut self.files_shared;
        let op = match request {
            Request::Clip { paths, cut } => {
                let count = paths.len();
                shared.clip = machine.map(|machine| Clip {
                    machine,
                    paths,
                    cut,
                });
                let verb = if cut { "cut" } else { "copied" };
                self.files_notice(win, format!("{count} {verb} to the clipboard"));
                return;
            }
            Request::Paste(to) => {
                let Some(clip) = shared.clip.clone() else {
                    self.files_notice(win, "the clipboard is empty".into());
                    return;
                };
                if Some(&clip.machine) != machine.as_ref() {
                    self.files_notice(win, "copying between Machines comes later".into());
                    return;
                }
                // A cut is pasted once; a copy as often as wanted.
                if clip.cut {
                    shared.clip = None;
                }
                Op::Paste {
                    paths: clip.paths,
                    to,
                    cut: clip.cut,
                }
            }
            Request::Undo => {
                let Some((on, pairs)) = shared.undo.pop() else {
                    self.files_notice(win, "nothing to undo".into());
                    return;
                };
                return self.files_spawn(win, on, Op::Undo(pairs));
            }
            Request::Ask(ask) => {
                self.dialog = Some(ask_dialog(win, ask));
                return;
            }
            Request::CopyPath(text) => {
                let text = text
                    .lines()
                    .map(|l| remote::absolute(machine.as_deref().unwrap_or(LOCAL_KEY), l.as_ref()))
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\n");
                let encoded = base64::engine::general_purpose::STANDARD.encode(&text);
                let mut out = std::io::stdout();
                let _ = write!(out, "\x1b]52;c;{encoded}\x07").and_then(|()| out.flush());
                self.files_notice(win, "path copied".into());
                return;
            }
            Request::Op(op) => op,
            _ => return,
        };
        if let Some(machine) = machine {
            self.files_spawn(win, machine, op);
        }
    }

    fn files_spawn(&mut self, win: WinId, machine: String, op: Op) {
        let remote = match self.target(&machine) {
            Some(Target::Machine(m)) => Some(m.clone()),
            Some(Target::Local) => None,
            None => return self.files_notice(win, format!("{machine} is not saved")),
        };
        ops::spawn(win, machine, remote, op, self.events.clone());
    }

    /// A dialog of [`ask_dialog`] was submitted with `text`.
    pub(super) fn files_answer(&mut self, win: WinId, ask: Ask, text: &str) {
        let op = match ask {
            Ask::Rename(from) => {
                let Some(dir) = from.parent() else {
                    return;
                };
                if !valid_name(text) {
                    return self.files_notice(win, format!("not a valid name: {text}"));
                }
                Op::Rename(from.clone(), dir.join(text))
            }
            Ask::NewFolder(dir) => {
                if !valid_name(text) {
                    return self.files_notice(win, format!("not a valid name: {text}"));
                }
                Op::NewFolder(dir.join(text))
            }
            Ask::Delete(paths) => Op::Delete(paths),
        };
        self.files_op_request(win, Request::Op(op));
    }

    pub(super) fn files_done(&mut self, win: WinId, done: Done) {
        if let Some(f) = self.files.get_mut(&win) {
            f.done(&done);
        }
        if !done.undo.is_empty() {
            self.files_shared.undo.push((done.machine, done.undo));
        }
        // Every window may show a changed folder; listing is cheap and off the UI thread.
        for other in self.files_windows() {
            self.files_request(other, Request::List);
        }
    }

    pub(super) fn files_notice(&mut self, win: WinId, text: String) {
        if let Some(f) = self.files.get_mut(&win) {
            f.notice = Some(text);
            f.error = None;
        }
    }
}

fn ask_dialog(win: WinId, ask: Ask) -> Dialog<DialogTarget> {
    let target = DialogTarget::Files(win, ask.clone());
    match ask {
        Ask::Rename(path) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Dialog::input("Rename", &name, "Rename", target)
        }
        Ask::NewFolder(_) => Dialog::input("New folder", "New folder", "Create", target),
        Ask::Delete(paths) => {
            let what = match paths.as_slice() {
                [one] => one
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                many => format!("{} items", many.len()),
            };
            let mut dialog = Dialog::confirm(
                "Delete permanently",
                format!("Delete {what}? This cannot be undone."),
                "Delete",
                target,
            );
            dialog.destructive = true;
            dialog
        }
    }
}

/// A single path component: not empty, `.` or `..`, and no separator.
fn valid_name(text: &str) -> bool {
    !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_one_path_component() {
        assert!(valid_name("notes (2).txt"));
        for bad in ["", ".", "..", "a/b", "a\\b"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
