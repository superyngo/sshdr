//! Copying between **Machines** (F5c): `tar -cf -` on the source piped through the desk into
//! `tar -xf -` on the destination, both in `sh` (locally, or over ssh). Each item lands in a
//! temporary folder first, then takes a free name, so a cancelled copy leaves nothing behind.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::ops::Done;
use super::remote::{self, PRELUDE};
use crate::cli::desk::stream::DeskEvent;
use crate::cli::desk::transport::{self, Machine};
use crate::cli::desk::wm::WinId;

/// Shared with the window: bytes so far, the estimated total, and the Cancel request.
#[derive(Debug, Default)]
pub(in crate::cli::desk) struct Progress {
    pub bytes: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

#[derive(Debug, Clone)]
pub(in crate::cli::desk) struct Copy {
    /// `None` is Local.
    pub from: Option<Machine>,
    pub paths: Vec<PathBuf>,
    pub to_machine: Option<Machine>,
    pub to: PathBuf,
}

const SIZE: &str = r#"for p; do du -sk -- "$(x "$p")"; done 2>/dev/null | awk '{s+=$1} END{print s+0}'
"#;
const PACK: &str = r#"p=$(x "$1"); cd -- "${p%/*}/" && tar -cf - -- "${p##*/}"
"#;
/// `$1` folder, `$2` name, `$3` token of the temporary folder.
const UNPACK: &str = r#"t=$(x "$1"); k="$t/.sshdr-copy-$3"
mkdir -- "$k" || exit 1
if tar -C "$k" -xf -; then f=$(free "$t" "$2"); mv -- "$k/$2" "$f" && rmdir -- "$k" && printf %s "$f"
else rm -rf -- "$k"; exit 1; fi
"#;
const CLEAN: &str = r#"rm -rf -- "$(x "$1")/.sshdr-copy-$2"
"#;

/// `sh -c` with [`PRELUDE`] on `machine`, or here.
fn sh(machine: Option<&Machine>, script: &str, args: &[&str]) -> io::Result<Command> {
    let script = format!("{PRELUDE}{script}");
    match machine {
        Some(machine) => transport::sh(machine, &script, args),
        None => {
            let mut command = Command::new("sh");
            command.args(["-c", &script, "sh"]).args(args);
            Ok(command)
        }
    }
}

pub(in crate::cli::desk) fn spawn(
    win: WinId,
    machine: String,
    copy: Copy,
    progress: Arc<Progress>,
    events: Sender<DeskEvent>,
) {
    let spawned = std::thread::Builder::new()
        .name("desk-files-copy".into())
        .spawn(move || {
            let mut done = run(&copy, &progress);
            done.machine = machine;
            let _ = events.send(DeskEvent::Done(win, Box::new(done)));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "desk: files copy thread failed to start");
    }
}

fn run(copy: &Copy, progress: &Progress) -> Done {
    let mut done = Done::default();
    let names: Vec<String> = copy
        .paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let args: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Ok(out) = sh(copy.from.as_ref(), SIZE, &args).and_then(output) {
        let kib = String::from_utf8_lossy(&out)
            .trim()
            .parse::<u64>()
            .unwrap_or(0);
        progress.total.store(kib * 1024, Ordering::Relaxed);
    }
    let mut count = 0;
    for path in &copy.paths {
        if progress.cancel.load(Ordering::Relaxed) {
            break;
        }
        match one(copy, path, progress) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(err) => {
                done.error
                    .get_or_insert_with(|| format!("{}: {err}", path.display()));
            }
        }
    }
    done.summary = if progress.cancel.load(Ordering::Relaxed) {
        done.error = None;
        format!("copy cancelled after {count}")
    } else {
        format!("{count} copied")
    };
    done
}

/// False when cancelled.
fn one(copy: &Copy, path: &Path, progress: &Progress) -> io::Result<bool> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("no name"))?
        .to_string_lossy()
        .into_owned();
    let source = path.to_string_lossy();
    let to = copy.to.to_string_lossy();
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let token = format!("{}-{token}", std::process::id());

    let mut pack = sh(copy.from.as_ref(), PACK, &[&source])?
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut unpack =
        match sh(copy.to_machine.as_ref(), UNPACK, &[&to, &name, &token]).and_then(|mut c| {
            c.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }) {
            Ok(child) => child,
            Err(err) => {
                stop(&mut pack);
                return Err(err);
            }
        };
    let pumped = pump(&mut pack, &mut unpack, progress);
    let cancelled = progress.cancel.load(Ordering::Relaxed);
    if cancelled || pumped.is_err() {
        stop(&mut pack);
        stop(&mut unpack);
        if let Ok(clean) = sh(copy.to_machine.as_ref(), CLEAN, &[&to, &token]) {
            let _ = output(clean);
        }
        return match pumped {
            Err(err) if !cancelled => Err(err),
            _ => Ok(false),
        };
    }
    let packed = pack.wait_with_output()?;
    let unpacked = unpack.wait_with_output()?;
    if !packed.status.success() {
        return Err(io::Error::other(last_line(&packed.stderr)));
    }
    if !unpacked.status.success() || unpacked.stdout.is_empty() {
        return Err(io::Error::other(last_line(&unpacked.stderr)));
    }
    Ok(true)
}

/// Source stdout into destination stdin, counting bytes; stops early on Cancel.
fn pump(pack: &mut Child, unpack: &mut Child, progress: &Progress) -> io::Result<()> {
    let (Some(mut from), Some(mut into)) = (pack.stdout.take(), unpack.stdin.take()) else {
        return Err(io::Error::other("no pipe"));
    };
    let mut buf = vec![0; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        into.write_all(&buf[..n])?;
        progress.bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn output(mut command: Command) -> io::Result<Vec<u8>> {
    let out = command.stdin(Stdio::null()).output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(io::Error::other(last_line(&out.stderr)))
    }
}

fn last_line(bytes: &[u8]) -> String {
    let text = remote::plain(&String::from_utf8_lossy(bytes));
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed")
        .to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn copies_through_tar_with_free_names_and_cancels_cleanly() {
        let dir = std::env::temp_dir().join(format!("desk-transfer-{}", std::process::id()));
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("sub/f.txt"), "hello").unwrap();
        std::fs::write(src.join("a b.txt"), "x").unwrap();
        std::fs::write(dst.join("a b.txt"), "y").unwrap();
        let copy = Copy {
            from: None,
            paths: vec![src.join("sub"), src.join("a b.txt")],
            to_machine: None,
            to: dst.clone(),
        };
        let progress = Progress::default();
        let done = run(&copy, &progress);
        assert_eq!((done.summary.as_str(), done.error), ("2 copied", None));
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/f.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("a b (2).txt")).unwrap(),
            "x"
        );
        assert!(progress.bytes.load(Ordering::Relaxed) > 0);

        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let done = run(&copy, &progress);
        assert_eq!(done.summary, "copy cancelled after 0");
        let left: Vec<_> = std::fs::read_dir(&dst)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".sshdr-copy"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
