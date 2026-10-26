//! Moving a path to the user's trash (F3b): `~/.Trash` on macOS, the freedesktop.org trash
//! (`$XDG_DATA_HOME/Trash` with a `.trashinfo` record) elsewhere on Unix. Only the home volume's
//! trash: a path on another volume fails rather than being copied. Windows is not supported yet.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use super::ops::free_name;

pub(super) fn trash(path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "no file name"))?;
    if cfg!(windows) {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "the trash is not supported on Windows yet",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "HOME is not set"))?;
    if cfg!(target_os = "macos") {
        let bin = home.join(".Trash");
        std::fs::create_dir_all(&bin)?;
        return no_copy(path, &free_name(&bin, &name));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    freedesktop(path, &data.join("Trash"), &name)
}

/// Trash into a freedesktop.org trash folder: `files/<name>` and `info/<name>.trashinfo`.
fn freedesktop(path: &Path, bin: &Path, name: &str) -> Result<()> {
    let (files, info) = (bin.join("files"), bin.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let target = free_name(&files, name);
    let kept = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let record = info.join(format!("{kept}.trashinfo"));
    let absolute = std::path::absolute(path)?;
    let date = crate::platform::local_datetime()
        .map(|t| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                t.year(),
                u8::from(t.month()),
                t.day(),
                t.hour(),
                t.minute(),
                t.second()
            )
        })
        .unwrap_or_default();
    std::fs::write(
        &record,
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={date}\n",
            encode(&absolute.to_string_lossy())
        ),
    )?;
    no_copy(path, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&record);
    })
}

/// A rename only: trashing never copies across volumes.
fn no_copy(path: &Path, target: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path).is_err() {
        return Err(Error::new(ErrorKind::NotFound, "not found"));
    }
    match std::fs::rename(path, target) {
        Err(err) if err.kind() == ErrorKind::CrossesDevices => Err(Error::new(
            ErrorKind::CrossesDevices,
            "on another volume than the trash",
        )),
        other => other,
    }
}

/// Percent-encode a path as the trash spec's `Path=` key wants (RFC 2396 path characters kept).
fn encode(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"/-_.~!$&'()*+,;=:@".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freedesktop_trash_keeps_a_record_and_counts_up_names() {
        let dir = std::env::temp_dir().join(format!("desk-trash-{}", std::process::id()));
        let bin = dir.join("Trash");
        std::fs::create_dir_all(&dir).ok();
        for _ in 0..2 {
            std::fs::write(dir.join("a b.txt"), "x").ok();
            freedesktop(&dir.join("a b.txt"), &bin, "a b.txt").ok();
        }
        assert!(bin.join("files/a b.txt").exists() && bin.join("files/a b (2).txt").exists());
        let info =
            std::fs::read_to_string(bin.join("info/a b (2).txt.trashinfo")).unwrap_or_default();
        assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
        assert!(info.contains("a%20b.txt\nDeletionDate=2"), "{info}");
        assert!(freedesktop(&dir.join("gone"), &bin, "gone").is_err());
        assert!(!bin.join("info/gone.trashinfo").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
