//! **Files** on a remote **Machine** (B17): POSIX `sh` over the desk's ssh ControlMaster, so
//! both Tiers work without a remote `herdr`. Remote paths keep a leading `~` for the home
//! folder; the scripts expand it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use super::listing::{kind_of, Entry, Listing};
use crate::cli::desk::stream::DeskEvent;
use crate::cli::desk::transport::{self, Machine};
use crate::cli::desk::wm::WinId;

/// `x` turns `~` paths into absolute ones (`p=$(x "$1")`); `free <folder> <name>` gives a
/// free ` (2)` name; `$F`/`$T` are the `stat` flag and format.
pub(super) const PRELUDE: &str = r#"x() { case $1 in "~") printf %s "$HOME";; "~/"*) printf %s "$HOME/${1#"~/"}";; *) printf %s "$1";; esac; }
free() {
  [ -e "$1/$2" ] || [ -L "$1/$2" ] || { printf %s "$1/$2"; return; }
  case $2 in ?*.*) b=${2%.*} e=.${2##*.};; *) b=$2 e=;; esac
  i=2; while [ -e "$1/$b ($i)$e" ] || [ -L "$1/$b ($i)$e" ]; do i=$((i+1)); done
  printf %s "$1/$b ($i)$e"
}
if stat -c %Y . >/dev/null 2>&1; then F=-c; T='%F|%s|%Y|%n'; else F=-f; T='%HT|%z|%m|%N'; fi
"#;

/// `$HOME`, free KiB, the folder's mtime, then one `type|size|mtime|./name` line per entry.
const LIST: &str = r#"cd -- "$(x "$1")" || exit 3
printf '%s\n' "$HOME"
df -Pk . 2>/dev/null | awk 'NR==2{print $4}'
stat -L $F "$T" . | cut -d'|' -f3
find . -mindepth 1 -maxdepth 1 -exec stat -L $F "$T" {} + 2>/dev/null
exit 0
"#;

/// Each **Machine**'s `$HOME`, learnt from listings, for callers that need absolute paths.
static HOMES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// `path` with its leading `~` replaced by the **Machine**'s home, once a listing learnt it.
pub(in crate::cli::desk) fn absolute(machine: &str, path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix(home()) else {
        return path.to_path_buf();
    };
    match HOMES.lock().ok().and_then(|h| h.get(machine).cloned()) {
        Some(home) => Path::new(&home).join(rest),
        None => path.to_path_buf(),
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);

/// The home folder of a **Machine**, as remote paths spell it.
pub(super) fn home() -> PathBuf {
    PathBuf::from("~")
}

pub(in crate::cli::desk) fn spawn(
    win: WinId,
    generation: u64,
    machine: Machine,
    path: PathBuf,
    events: Sender<DeskEvent>,
) {
    let spawned = std::thread::Builder::new()
        .name("desk-files-remote".into())
        .spawn(move || {
            let listing = list(&machine, &path, generation);
            let _ = events.send(DeskEvent::Listed(win, Box::new(listing)));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "desk: remote files listing thread failed to start");
    }
}

/// Run `script` (after [`PRELUDE`]) with `args`; stdout, or stderr as the error.
pub(super) fn run(machine: &Machine, script: &str, args: &[&str]) -> Result<String, String> {
    let command = transport::sh(machine, &format!("{PRELUDE}{script}"), args)
        .map_err(|err| format!("ssh: {err}"))?;
    let out = transport::output_within(command, TIMEOUT).map_err(|err| format!("ssh: {err}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = plain(&String::from_utf8_lossy(&out.stderr));
    // PowerShell (the default ssh shell on Windows) cannot run the `sh -c` line.
    let powershell = ["is not recognized", "ParserError", "FullyQualifiedErrorId"];
    if powershell.iter().any(|p| err.contains(p)) {
        return Err(format!(
            "{}: Windows Machines are not supported yet",
            machine.label
        ));
    }
    let err = err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Err(match out.status.code() {
        Some(255) => format!("{} unreachable: {err}", machine.label),
        _ if err.is_empty() => format!("failed ({})", out.status),
        _ => err.to_string(),
    })
}

/// Text without ANSI escapes or carriage returns (PowerShell colors its errors).
pub(super) fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.next() == Some('[') {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn list(machine: &Machine, path: &Path, generation: u64) -> Listing {
    let text = path.to_string_lossy();
    match run(machine, LIST, &[&text]) {
        Ok(out) => {
            let (home, rest) = out.split_once('\n').unwrap_or_default();
            if let Ok(mut homes) = HOMES.lock() {
                homes.insert(machine.id.clone(), home.to_string());
            }
            let (free, stamp, entries) = parse(rest);
            Listing {
                generation,
                entries: Ok(entries),
                stamp,
                free,
            }
        }
        Err(err) => Listing {
            generation,
            entries: Err(format!("{text}: {err}")),
            stamp: None,
            free: None,
        },
    }
}

fn time(secs: &str) -> Option<SystemTime> {
    let secs = secs.trim().parse::<u64>().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// Lines that do not parse (a name with a newline) are skipped.
fn parse(out: &str) -> (Option<u64>, Option<SystemTime>, Vec<Entry>) {
    let mut lines = out.lines();
    let free = lines
        .next()
        .and_then(|l| l.trim().parse::<u64>().ok())
        .map(|k| k.saturating_mul(1024));
    let stamp = lines.next().and_then(time);
    let entries = lines
        .filter_map(|line| {
            let mut parts = line.splitn(4, '|');
            let (kind, size, modified, name) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            let name = name.strip_prefix("./")?.to_string();
            let dir = kind.eq_ignore_ascii_case("directory");
            Some(Entry {
                kind: kind_of(&name, dir),
                size: if dir { 0 } else { size.parse().unwrap_or(0) },
                modified: time(modified),
                dir,
                name,
            })
        })
        .collect();
    (free, stamp, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gnu_and_bsd_stat() {
        let gnu = "1000\n1700000000\ndirectory|4096|1700000001|./src\nregular file|12|1700000002|./a|b.txt\n";
        let (free, stamp, entries) = parse(gnu);
        assert_eq!(free, Some(1_024_000));
        assert!(stamp.is_some());
        assert_eq!(entries.len(), 2);
        assert!(entries[0].dir && entries[0].size == 0 && entries[0].kind == "folder");
        assert_eq!((entries[1].name.as_str(), entries[1].size), ("a|b.txt", 12));
        let bsd = "\n1\nDirectory|64|2|./d\nRegular File|3|4|./.zshrc\nbroken line\n";
        let (free, _, entries) = parse(bsd);
        assert_eq!(free, None);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].dir);
        assert_eq!(entries[1].kind, "file");
    }

    #[test]
    fn escapes_are_dropped() {
        assert_eq!(plain("\x1b[31;1msh: bad\x1b[0m\r\n"), "sh: bad\n");
    }

    #[cfg(unix)]
    #[test]
    fn list_script_runs_in_a_posix_shell() {
        let dir = std::env::temp_dir().join(format!("desk-remote-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a b.txt"), "hi").unwrap();
        let out = std::process::Command::new("sh")
            .args(["-c", &format!("{PRELUDE}{LIST}"), "sh"])
            .arg(&dir)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        let (home, rest) = out.split_once('\n').unwrap();
        assert_eq!(Some(home.into()), std::env::var("HOME").ok());
        let (_, stamp, mut entries) = parse(rest);
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert!(stamp.is_some());
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].name.as_str(), entries[0].size), ("a b.txt", 2));
        assert!(entries[1].dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
