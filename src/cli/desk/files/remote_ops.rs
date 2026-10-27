//! **Files** operations on a remote **Machine** (F5b): one `sh` script per operation over ssh.
//! Each success prints `O`, followed by `now<TAB>was` when it can be undone; each failure prints
//! `E` and its message. Same behaviour as the Local [`super::ops`].

use std::path::PathBuf;

use super::ops::{Done, Op};
use super::remote;
use crate::cli::desk::transport::Machine;

const OPS: &str = r#"free() {
  [ -e "$1/$2" ] || [ -L "$1/$2" ] || { printf %s "$1/$2"; return; }
  case $2 in ?*.*) b=${2%.*} e=.${2##*.};; *) b=$2 e=;; esac
  i=2; while [ -e "$1/$b ($i)$e" ] || [ -L "$1/$b ($i)$e" ]; do i=$((i+1)); done
  printf %s "$1/$b ($i)$e"
}
enc() {
  printf %s "$1" | od -An -v -tx1 | tr -s ' ' '\n' | while read -r h; do
    case $h in
      '') ;;
      2[146-9a-f]|3[0-9abd]|4[0-9a-f]|5[0-9a]|5f|6[1-9a-f]|7[0-9a]|7e) printf "\\$(printf %03o "0x$h")";;
      *) printf %%%s "$h" | tr a-f A-F;;
    esac
  done
}
did() { if r=$("$@" 2>&1); then printf 'O%s\n' "$u"; else printf 'E%s\n' "$r" | head -n 1; fi; }
op=$1; shift
case $op in
paste)
  to=$(x "$1"); cut=$2; shift 2
  for p; do
    p=$(x "$p"); n=${p##*/}
    case $to/ in "$p"/*) printf 'E%s cannot go into itself\n' "$p"; continue;; esac
    t=$(free "$to" "$n")
    if [ "$cut" = 1 ]; then u="$t	$p"; did mv -- "$p" "$t"; else u=; did cp -Rp -- "$p" "$t"; fi
  done;;
rename)
  f=$(x "$1"); t=$(x "$2")
  if [ -e "$t" ] || [ -L "$t" ]; then printf 'E%s already exists\n' "$t"; else u="$t	$f"; did mv -- "$f" "$t"; fi;;
mkdir) u=; did mkdir -- "$(x "$1")";;
delete) u=; for p; do did rm -rf -- "$(x "$p")"; done;;
undo)
  while [ $# -ge 2 ]; do
    f=$(x "$1") t=$(x "$2"); shift 2; u=
    if [ -e "$t" ] || [ -L "$t" ]; then printf 'E%s already exists\n' "$t"; else did mv -- "$f" "$t"; fi
  done;;
trash)
  if [ "$(uname)" = Darwin ]; then T=$HOME/.Trash; mkdir -p "$T"; else
    T=${XDG_DATA_HOME:-$HOME/.local/share}/Trash; mkdir -p "$T/files" "$T/info"; fi
  for p; do
    p=$(x "$p"); u=
    case $p in /*) ;; *) p=$PWD/$p;; esac
    if [ "$(uname)" = Darwin ]; then did mv -- "$p" "$(free "$T" "${p##*/}")"; continue; fi
    t=$(free "$T/files" "${p##*/}"); i="$T/info/${t##*/}.trashinfo"
    printf '[Trash Info]\nPath=%s\nDeletionDate=%s\n' "$(enc "$p")" "$(date +%Y-%m-%dT%H:%M:%S)" >"$i"
    did mv -- "$p" "$t"
    [ -e "$t" ] || [ -L "$t" ] || rm -f "$i"
  done;;
esac
"#;

/// The script's subcommand and arguments for `op`.
fn args(op: &Op) -> Vec<String> {
    let s = |p: &PathBuf| p.to_string_lossy().into_owned();
    let mut out = Vec::new();
    match op {
        Op::Paste { paths, to, cut } => {
            out.extend(["paste".into(), s(to), if *cut { "1" } else { "0" }.into()]);
            out.extend(paths.iter().map(s));
        }
        Op::Rename(from, to) => out.extend(["rename".into(), s(from), s(to)]),
        Op::NewFolder(path) => out.extend(["mkdir".into(), s(path)]),
        Op::Trash(paths) => {
            out.push("trash".into());
            out.extend(paths.iter().map(s));
        }
        Op::Delete(paths) => {
            out.push("delete".into());
            out.extend(paths.iter().map(s));
        }
        Op::Undo(pairs) => {
            out.push("undo".into());
            for (from, to) in pairs.iter().rev() {
                out.extend([s(from), s(to)]);
            }
        }
    }
    out
}

fn summary(op: &Op, count: usize) -> String {
    match op {
        Op::Paste { cut: true, .. } => format!("{count} moved"),
        Op::Paste { .. } => format!("{count} copied"),
        Op::Rename(..) => "renamed".into(),
        Op::NewFolder(_) => "folder created".into(),
        Op::Trash(_) => format!("{count} moved to the trash"),
        Op::Delete(_) => format!("{count} deleted"),
        Op::Undo(_) => format!("{count} undone"),
    }
}

fn parse(op: &Op, out: &str) -> Done {
    let mut done = Done::default();
    let mut count = 0;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix('O') {
            count += 1;
            if let Some((now, was)) = rest.split_once('\t') {
                done.undo.push((now.into(), was.into()));
            }
        } else if let Some(err) = line.strip_prefix('E') {
            done.error.get_or_insert_with(|| err.to_string());
        }
    }
    if matches!(op, Op::Rename(..) | Op::NewFolder(_)) && count == 0 {
        done.summary = String::new();
    } else {
        done.summary = summary(op, count);
    }
    done
}

pub(in crate::cli::desk) fn run(machine: &Machine, op: &Op) -> Done {
    let args = args(op);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match remote::run(machine, OPS, &args) {
        Ok(out) => parse(op, &out),
        Err(err) => Done {
            error: Some(err),
            ..Done::default()
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;

    fn local(home: &Path, op: &Op) -> Done {
        let out = std::process::Command::new("sh")
            .args(["-c", &format!("{}{OPS}", remote::PRELUDE), "sh"])
            .args(args(op))
            .env("HOME", home)
            .env_remove("XDG_DATA_HOME")
            .output()
            .unwrap();
        parse(op, &String::from_utf8_lossy(&out.stdout))
    }

    #[test]
    fn the_script_copies_moves_renames_undoes_trashes_and_deletes() {
        let home = std::env::temp_dir().join(format!("desk-remote-ops-{}", std::process::id()));
        let (src, dst) = (home.join("src"), home.join("dst"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a b.txt"), "x").unwrap();
        std::fs::write(dst.join("a b.txt"), "y").unwrap();

        let copy = Op::Paste {
            paths: vec!["~/src/a b.txt".into(), "~/src/sub".into()],
            to: "~/dst".into(),
            cut: false,
        };
        let done = local(&home, &copy);
        assert_eq!((done.summary.as_str(), done.error), ("2 copied", None));
        assert!(dst.join("a b (2).txt").exists() && dst.join("sub").is_dir());

        let moved = local(
            &home,
            &Op::Paste {
                paths: vec![src.join("sub")],
                to: dst.join("sub"),
                cut: true,
            },
        );
        assert_eq!(moved.summary, "1 moved");
        assert_eq!(moved.undo, [(dst.join("sub/sub"), src.join("sub"))]);
        let undone = local(&home, &Op::Undo(moved.undo));
        assert_eq!((undone.summary.as_str(), undone.error), ("1 undone", None));
        assert!(src.join("sub").is_dir());

        let into_itself = local(
            &home,
            &Op::Paste {
                paths: vec![src.clone()],
                to: src.join("sub"),
                cut: true,
            },
        );
        assert!(into_itself.error.unwrap().contains("into itself"));

        let renamed = local(&home, &Op::Rename(dst.join("sub"), dst.join("sub")));
        assert!(renamed.error.unwrap().contains("already exists"));
        let made = local(&home, &Op::NewFolder("~/dst/new".into()));
        assert_eq!(made.summary, "folder created");

        let trashed = local(&home, &Op::Trash(vec![dst.join("a b (2).txt")]));
        assert_eq!(
            (trashed.summary.as_str(), trashed.error),
            ("1 moved to the trash", None)
        );
        assert!(!dst.join("a b (2).txt").exists());
        if !cfg!(target_os = "macos") {
            let info =
                std::fs::read_to_string(home.join(".local/share/Trash/info/a b (2).txt.trashinfo"))
                    .unwrap();
            assert!(info.contains("a%20b%20(2).txt\nDeletionDate="), "{info}");
        }

        let deleted = local(&home, &Op::Delete(vec![dst.clone()]));
        assert_eq!(deleted.summary, "1 deleted");
        assert!(!dst.exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}
