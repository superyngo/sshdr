//! File operations of a **Files** window (F3b), Local here, remote in [`super::remote_ops`]: paste (copy or move), rename, new
//! folder, trash, and permanent delete. Each runs on a short-lived thread like a listing; the
//! result comes back as [`DeskEvent::Done`]. Moves and renames return what undoes them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use super::super::stream::DeskEvent;
use super::super::transport::Machine;
use super::super::wm::WinId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::cli::desk) enum Op {
    /// Copy, or move when `cut`, every path into the folder `to`.
    Paste {
        paths: Vec<PathBuf>,
        to: PathBuf,
        cut: bool,
    },
    Rename(PathBuf, PathBuf),
    NewFolder(PathBuf),
    Trash(Vec<PathBuf>),
    Delete(Vec<PathBuf>),
    /// Move each `(from, to)` back: the undo of a move or rename.
    Undo(Vec<(PathBuf, PathBuf)>),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::cli::desk) struct Done {
    /// What was done, for the status bar.
    pub summary: String,
    /// The first failure; the other paths were still tried.
    pub error: Option<String>,
    /// `(now, was)` pairs that undo this operation.
    pub undo: Vec<(PathBuf, PathBuf)>,
    /// The **Machine** it ran on.
    pub machine: String,
}

/// Run `op` on **Machine** `machine` (`remote` unless Local) off the UI thread.
pub(in crate::cli::desk) fn spawn(
    win: WinId,
    machine: String,
    remote: Option<Machine>,
    op: Op,
    events: Sender<DeskEvent>,
) {
    let spawned = std::thread::Builder::new()
        .name("desk-files-op".into())
        .spawn(move || {
            let mut done = match &remote {
                Some(remote) => super::remote_ops::run(remote, &op),
                None => run(op),
            };
            done.machine = machine;
            let _ = events.send(DeskEvent::Done(win, Box::new(done)));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "desk: files operation thread failed to start");
    }
}

pub(in crate::cli::desk) fn run(op: Op) -> Done {
    let mut done = Done::default();
    let fail = |done: &mut Done, path: &Path, err: std::io::Error| {
        done.error
            .get_or_insert_with(|| format!("{}: {err}", path.display()));
    };
    match op {
        Op::Paste { paths, to, cut } => {
            let mut count = 0;
            for path in &paths {
                let Some(name) = path.file_name() else {
                    continue;
                };
                if to.starts_with(path) {
                    done.error = Some(format!("{} cannot go into itself", path.display()));
                    continue;
                }
                let target = free_name(&to, &name.to_string_lossy());
                let result = if cut {
                    move_path(path, &target)
                } else {
                    copy_tree(path, &target)
                };
                match result {
                    Ok(()) => {
                        count += 1;
                        if cut {
                            done.undo.push((target, path.clone()));
                        }
                    }
                    Err(err) => fail(&mut done, path, err),
                }
            }
            let verb = if cut { "moved" } else { "copied" };
            done.summary = format!("{count} {verb}");
        }
        Op::Rename(from, to) => {
            if to.exists() {
                done.error = Some(format!("{} already exists", to.display()));
            } else {
                match std::fs::rename(&from, &to) {
                    Ok(()) => {
                        done.summary = "renamed".into();
                        done.undo.push((to, from));
                    }
                    Err(err) => fail(&mut done, &from, err),
                }
            }
        }
        Op::NewFolder(path) => match std::fs::create_dir(&path) {
            Ok(()) => done.summary = "folder created".into(),
            Err(err) => fail(&mut done, &path, err),
        },
        Op::Trash(paths) => {
            let mut count = 0;
            for path in &paths {
                match super::trash::trash(path) {
                    Ok(()) => count += 1,
                    Err(err) => fail(&mut done, path, err),
                }
            }
            done.summary = format!("{count} moved to the trash");
        }
        Op::Delete(paths) => {
            let mut count = 0;
            for path in &paths {
                let result = match std::fs::symlink_metadata(path) {
                    Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
                    Ok(_) => std::fs::remove_file(path),
                    Err(err) => Err(err),
                };
                match result {
                    Ok(()) => count += 1,
                    Err(err) => fail(&mut done, path, err),
                }
            }
            done.summary = format!("{count} deleted");
        }
        Op::Undo(pairs) => {
            let mut count = 0;
            for (from, to) in pairs.iter().rev() {
                if to.exists() {
                    done.error = Some(format!("{} already exists", to.display()));
                    continue;
                }
                match move_path(from, to) {
                    Ok(()) => count += 1,
                    Err(err) => fail(&mut done, from, err),
                }
            }
            done.summary = format!("{count} undone");
        }
    }
    done
}

/// `name`, or `name (2).ext`, `name (3).ext`… whichever does not exist in `dir`.
pub(in crate::cli::desk) fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => name.split_at(at),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .unwrap_or(first)
}

/// Rename, or copy then remove across volumes.
pub(in crate::cli::desk) fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_tree(from, to)?;
            if std::fs::symlink_metadata(from)?.is_dir() {
                std::fs::remove_dir_all(from)
            } else {
                std::fs::remove_file(from)
            }
        }
        Err(err) => Err(err),
    }
}

/// Copy a file or a folder with everything in it; links are copied as links.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return copy_link(from, to);
    }
    if !meta.is_dir() {
        return std::fs::copy(from, to).map(|_| ());
    }
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

/// No unprivileged links on Windows: copy what the link points at.
#[cfg(windows)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "desk-ops-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn free_names_count_up_before_the_extension() {
        let dir = play();
        std::fs::write(dir.join("a.txt"), "x").ok();
        std::fs::write(dir.join("a (2).txt"), "x").ok();
        assert_eq!(free_name(&dir, "a.txt"), dir.join("a (3).txt"));
        assert_eq!(free_name(&dir, "b"), dir.join("b"));
        std::fs::create_dir_all(dir.join(".rc")).ok();
        assert_eq!(free_name(&dir, ".rc"), dir.join(".rc (2)"));
    }

    #[test]
    fn copy_move_rename_undo_and_delete() {
        let dir = play().join("ops");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::create_dir_all(src.join("sub")).ok();
        std::fs::create_dir_all(&dst).ok();
        std::fs::write(src.join("sub/f.txt"), "hi").ok();
        let copied = run(Op::Paste {
            paths: vec![src.join("sub")],
            to: dst.clone(),
            cut: false,
        });
        assert_eq!((copied.summary.as_str(), copied.error), ("1 copied", None));
        assert!(dst.join("sub/f.txt").exists() && src.join("sub").exists());
        let into_itself = run(Op::Paste {
            paths: vec![src.clone()],
            to: src.join("sub"),
            cut: true,
        });
        assert!(into_itself.error.is_some());
        let moved = run(Op::Paste {
            paths: vec![src.join("sub")],
            to: dst.clone(),
            cut: true,
        });
        assert!(dst.join("sub (2)/f.txt").exists() && !src.join("sub").exists());
        let undone = run(Op::Undo(moved.undo));
        assert_eq!(undone.summary, "1 undone");
        assert!(src.join("sub/f.txt").exists());
        let renamed = run(Op::Rename(dst.join("sub"), dst.join("renamed")));
        assert!(renamed.error.is_none() && dst.join("renamed").exists());
        let clash = run(Op::Rename(dst.join("renamed"), src.clone()));
        assert!(clash.error.is_some());
        let deleted = run(Op::Delete(vec![dir.clone()]));
        assert_eq!(deleted.summary, "1 deleted");
        assert!(!dir.exists());
    }
}
