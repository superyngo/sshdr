//! sshdr: **Machines** saved with `machine add --ssh-only` (B11): hosts kept without herdr
//! setup. The file records only how they were saved; their **Tier** is still detected on each
//! connection (ADR 0004 decisions 2 and 5).
//!
//! Kept in their own file, `<state>/client/ssh-machines.json`, so herdr's endpoint catalog
//! (`deny_unknown_fields`, read by every herdr client and older versions) stays unchanged and
//! herdr clients never try to connect to these hosts. Labels share one namespace with the
//! catalog.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::endpoint::{EndpointCatalog, ProfileId, SavedSshEndpoint};

const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SshMachine {
    pub id: ProfileId,
    pub label: String,
    pub target: String,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    #[serde(default)]
    machines: Vec<SshMachine>,
}

pub(crate) fn path() -> PathBuf {
    crate::config::state_dir()
        .join("client")
        .join("ssh-machines.json")
}

/// Saved SSH-only **Machines**; a missing file is an empty list.
pub(crate) fn load() -> Result<Vec<SshMachine>, String> {
    load_from(&path())
}

fn load_from(path: &Path) -> Result<Vec<SshMachine>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let file: File = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    if file.version != VERSION {
        return Err(format!(
            "unsupported {} version {}",
            path.display(),
            file.version
        ));
    }
    Ok(file.machines)
}

fn store_to(path: &Path, machines: &[SshMachine]) -> Result<(), String> {
    let file = File {
        version: VERSION,
        machines: machines.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// Whether `label` is taken by a catalog profile or an SSH-only **Machine**.
pub(crate) fn label_taken(catalog: &EndpointCatalog, machines: &[SshMachine], label: &str) -> bool {
    catalog.ssh.iter().any(|profile| profile.label == label)
        || machines.iter().any(|machine| machine.label == label)
}

/// Validated like a catalog profile (label, target, no password); returns the new id.
fn add_to(
    path: &Path,
    catalog: &EndpointCatalog,
    label: &str,
    target: &str,
) -> Result<ProfileId, String> {
    let mut machines = load_from(path)?;
    if label_taken(catalog, &machines, label) {
        return Err(format!("a machine named '{label}' already exists"));
    }
    let checked = SavedSshEndpoint::new(
        label,
        target,
        crate::session::DEFAULT_SESSION_NAME.to_owned(),
    )?;
    machines.push(SshMachine {
        id: checked.id.clone(),
        label: checked.label,
        target: checked.target,
    });
    store_to(path, &machines)?;
    Ok(checked.id)
}

pub(crate) fn add(
    catalog: &EndpointCatalog,
    label: &str,
    target: &str,
) -> Result<ProfileId, String> {
    add_to(&path(), catalog, label, target)
}

/// `Ok(false)` when `id` is not an SSH-only **Machine**.
pub(crate) fn remove(id: &ProfileId) -> Result<bool, String> {
    let path = path();
    let mut machines = load_from(&path)?;
    let before = machines.len();
    machines.retain(|machine| &machine.id != id);
    if machines.len() == before {
        return Ok(false);
    }
    store_to(&path, &machines)?;
    Ok(true)
}

/// `Ok(false)` when `id` is not an SSH-only **Machine**.
pub(crate) fn rename(
    catalog: &EndpointCatalog,
    id: &ProfileId,
    label: &str,
) -> Result<bool, String> {
    let path = path();
    let mut machines = load_from(&path)?;
    let Some(index) = machines.iter().position(|machine| &machine.id == id) else {
        return Ok(false);
    };
    let others: Vec<SshMachine> = machines
        .iter()
        .filter(|machine| &machine.id != id)
        .cloned()
        .collect();
    if label_taken(catalog, &others, label) {
        return Err(format!("a machine named '{label}' already exists"));
    }
    SavedSshEndpoint::new(
        label,
        machines[index].target.clone(),
        crate::session::DEFAULT_SESSION_NAME.to_owned(),
    )?;
    machines[index].label = label.to_owned();
    store_to(&path, &machines)?;
    Ok(true)
}

/// Whether plain SSH to `target` works without a prompt (`BatchMode`). ssh exits 255 for its
/// own failures; any other status came from the remote shell.
pub(crate) fn ssh_works(target: &str) -> bool {
    std::process::Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-T"])
        .arg(target)
        .arg("exit")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.code().is_some_and(|code| code != 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ssh-machines-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("ssh-machines.json")
    }

    #[test]
    fn machines_round_trip_and_labels_stay_unique() {
        let path = temp("roundtrip");
        let catalog = EndpointCatalog::default();
        assert_eq!(load_from(&path), Ok(Vec::new()));
        let id = add_to(&path, &catalog, "smbnfs", "Admin@smbnfs").expect("add");
        let machines = load_from(&path).expect("load");
        assert_eq!(machines.len(), 1);
        assert_eq!(
            (machines[0].id.clone(), machines[0].label.as_str()),
            (id, "smbnfs")
        );
        assert!(add_to(&path, &catalog, "smbnfs", "other").is_err());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn invalid_targets_and_files_are_rejected() {
        let path = temp("invalid");
        let catalog = EndpointCatalog::default();
        assert!(add_to(&path, &catalog, "x", "user:secret@host").is_err());
        assert!(add_to(&path, &catalog, "", "host").is_err());
        std::fs::write(&path, br#"{"version":1,"machines":[],"extra":1}"#).expect("write");
        assert!(load_from(&path).is_err());
    }
}
