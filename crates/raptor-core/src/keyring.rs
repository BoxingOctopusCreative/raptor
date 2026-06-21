use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::fs_util::{move_file, temp_file_in};

/// Return true when `bytes` look like an armored ASCII PGP key block.
pub fn is_armored_key(bytes: &[u8]) -> bool {
    bytes.starts_with(b"-----BEGIN PGP")
}

/// Ensure `key_path` is a gpgv-compatible keyring, dearmoring armored keys when needed.
///
/// Armored `.asc` files are written to a sibling `.gpg` path. Armored content stored under
/// another extension is dearmored in place.
pub fn ensure_dearmored_keyring(key_path: &Path) -> Result<PathBuf> {
    let bytes = fs::read(key_path).map_err(|e| {
        Error::SignatureVerification(format!(
            "could not read signing key {}: {e}",
            key_path.display()
        ))
    })?;

    if !is_armored_key(&bytes) {
        return Ok(key_path.to_path_buf());
    }

    if !has_command("gpg") {
        return Err(Error::SignatureVerification(
            "gpg is required to dearmor the signing key".into(),
        ));
    }

    let gpg_path = dearmored_keyring_path(key_path);
    if let Some(parent) = gpg_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = if gpg_path == key_path {
        temp_file_in(
            gpg_path.parent().unwrap_or_else(|| Path::new("/tmp")),
            "keyring",
        )?
    } else {
        gpg_path.clone()
    };

    let status = Command::new("gpg")
        .args([
            "--batch",
            "--yes",
            "--dearmor",
            "--output",
            output.to_str().unwrap_or_default(),
            key_path.to_str().unwrap_or_default(),
        ])
        .status()
        .map_err(|e| Error::SignatureVerification(format!("failed to run gpg: {e}")))?;

    if !status.success() {
        let _ = fs::remove_file(&output);
        return Err(Error::SignatureVerification(format!(
            "gpg --dearmor failed for {}",
            key_path.display()
        )));
    }

    if output != gpg_path {
        move_file(&output, &gpg_path)?;
    }

    Ok(gpg_path)
}

fn dearmored_keyring_path(key_path: &Path) -> PathBuf {
    match key_path.extension().and_then(|ext| ext.to_str()) {
        Some("asc") | Some("ASC") => key_path.with_extension("gpg"),
        _ => key_path.to_path_buf(),
    }
}

fn has_command(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_armored_keys() {
        assert!(is_armored_key(b"-----BEGIN PGP PUBLIC KEY BLOCK-----\n"));
        assert!(!is_armored_key(b"\x99\x01\x04"));
    }

    #[test]
    fn passes_through_binary_keyring() {
        let dir = std::env::temp_dir().join(format!("raptor-keyring-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let key = dir.join("repo.gpg");
        fs::write(&key, b"\x99\x01\x04binary").unwrap();

        assert_eq!(ensure_dearmored_keyring(&key).unwrap(), key);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dearmors_asc_to_gpg() {
        if !has_command("gpg") {
            return;
        }

        let system_key = Path::new("/usr/share/keyrings/ubuntu-archive-keyring.gpg");
        if !system_key.is_file() {
            return;
        }

        let dir = std::env::temp_dir().join(format!("raptor-keyring-asc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let asc = dir.join("repo.asc");
        let armored = Command::new("gpg")
            .args([
                "--batch",
                "--yes",
                "--armor",
                "--export",
                "--no-default-keyring",
                "--keyring",
                system_key.to_str().unwrap_or_default(),
            ])
            .output()
            .expect("gpg --export");
        assert!(armored.status.success());
        fs::write(&asc, &armored.stdout).unwrap();

        let gpg = ensure_dearmored_keyring(&asc).unwrap();
        assert_eq!(gpg, dir.join("repo.gpg"));
        assert!(gpg.is_file());
        assert!(!is_armored_key(&fs::read(&gpg).unwrap()));

        let _ = fs::remove_dir_all(&dir);
    }
}
