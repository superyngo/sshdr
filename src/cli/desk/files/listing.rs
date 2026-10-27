//! Reading a folder off the UI thread: one short-lived thread per listing, its result sent to
//! the event loop as [`DeskEvent::Listed`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use super::super::stream::DeskEvent;
use super::super::wm::WinId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::cli::desk) struct Entry {
    pub name: String,
    /// A folder, or a link to one.
    pub dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// `folder`, the lowercase extension, or `file`.
    pub kind: String,
}

#[derive(Debug)]
pub(in crate::cli::desk) struct Listing {
    pub generation: u64,
    pub entries: Result<Vec<Entry>, String>,
    /// The folder's own modification time.
    pub stamp: Option<SystemTime>,
    /// Bytes free for the user on the folder's volume.
    pub free: Option<u64>,
}

pub(in crate::cli::desk) fn spawn(
    win: WinId,
    generation: u64,
    path: PathBuf,
    events: Sender<DeskEvent>,
) {
    let spawned = std::thread::Builder::new()
        .name("desk-files-list".into())
        .spawn(move || {
            let listing = list(&path, generation);
            let _ = events.send(DeskEvent::Listed(win, Box::new(listing)));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "desk: files listing thread failed to start");
    }
}

pub(in crate::cli::desk) fn list(path: &Path, generation: u64) -> Listing {
    let stamp = modified(path);
    let entries = std::fs::read_dir(path)
        .map(|dir| dir.filter_map(Result::ok).map(|e| entry(&e)).collect())
        .map_err(|err| format!("{}: {err}", path.display()));
    Listing {
        generation,
        entries,
        stamp,
        free: free_space(path),
    }
}

/// The folder's modification time; a change means entries were added, removed, or renamed.
pub(in crate::cli::desk) fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn entry(e: &std::fs::DirEntry) -> Entry {
    let name = e.file_name().to_string_lossy().into_owned();
    // Follow links for the kind and size; a broken link stays a file.
    let meta = std::fs::metadata(e.path()).or_else(|_| e.metadata()).ok();
    let dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
    let kind = kind_of(&name, dir);
    Entry {
        dir,
        size: if dir {
            0
        } else {
            meta.as_ref().map_or(0, std::fs::Metadata::len)
        },
        modified: meta.and_then(|m| m.modified().ok()),
        kind,
        name,
    }
}

/// `folder`, the lowercase extension, or `file`.
pub(super) fn kind_of(name: &str, dir: bool) -> String {
    if dir {
        return "folder".to_string();
    }
    Path::new(name)
        .extension()
        .filter(|_| !name.starts_with('.') || name.matches('.').count() > 1)
        .map_or_else(
            || "file".to_string(),
            |x| x.to_string_lossy().to_lowercase(),
        )
}

#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `stat` a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::useless_conversion)] // Field widths differ between platforms.
    Some(u64::from(stat.f_bavail).saturating_mul(u64::from(stat.f_frsize)))
}

#[cfg(not(unix))]
fn free_space(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_files_and_kinds() {
        let dir = std::env::temp_dir().join(format!("desk-files-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.MD"), b"hello").unwrap();
        std::fs::write(dir.join(".rc"), b"").unwrap();
        let listing = list(&dir, 7);
        let mut entries = listing.entries.unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let got: Vec<(&str, bool, u64, &str)> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.dir, e.size, e.kind.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (".rc", false, 0, "file"),
                ("notes.MD", false, 5, "md"),
                ("sub", true, 0, "folder")
            ]
        );
        assert_eq!(listing.generation, 7);
        assert!(listing.stamp.is_some());
        assert!(list(&dir.join("missing"), 1).entries.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
