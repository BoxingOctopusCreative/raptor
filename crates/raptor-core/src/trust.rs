use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::repository::PackageIndexEntry;

const TRUST_SUFFIX: &str = ".raptor-trust";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustRecord {
    pub source_uri: String,
    pub suite: String,
    pub component: String,
    pub arch: String,
    pub keyring: String,
    pub packages_sha256: String,
    pub verified_at_secs: u64,
}

pub fn trust_path_for_packages(packages_path: &Path) -> PathBuf {
    PathBuf::from(format!(
        "{}{}",
        packages_path.display(),
        TRUST_SUFFIX
    ))
}

pub fn write_trust_record(packages_path: &Path, record: &TrustRecord) -> Result<()> {
    let path = trust_path_for_packages(packages_path);
    let json = serde_json::to_string_pretty(record)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, json)?;
    Ok(())
}

pub fn load_trust_record(packages_path: &Path) -> Result<TrustRecord> {
    let path = trust_path_for_packages(packages_path);
    let content = fs::read_to_string(&path).map_err(|e| {
        Error::SignatureVerification(format!(
            "no GPG trust record for {}; run raptor repo update: {e}",
            packages_path.display()
        ))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        Error::SignatureVerification(format!(
            "invalid trust record for {}: {e}",
            packages_path.display()
        ))
    })
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Confirm the cached `Packages` index still matches a GPG-verified update session.
pub fn verify_packages_trust(entry: &PackageIndexEntry) -> Result<()> {
    let packages_path = entry.packages_index_path.as_ref().ok_or_else(|| {
        Error::SignatureVerification(format!(
            "package {} has no trusted index provenance; run raptor repo update",
            entry.control.package
        ))
    })?;

    let record = load_trust_record(packages_path)?;

    if entry.source_uri.as_deref() != Some(record.source_uri.as_str()) {
        return Err(Error::SignatureVerification(format!(
            "trust record source mismatch for {}",
            entry.control.package
        )));
    }

    if !Path::new(&record.keyring).exists() {
        return Err(Error::SignatureVerification(format!(
            "keyring not found: {}",
            record.keyring
        )));
    }

    if entry.signed_by.as_deref() != Some(record.keyring.as_str()) {
        return Err(Error::SignatureVerification(format!(
            "keyring mismatch for {}",
            entry.control.package
        )));
    }

    let current_sha = sha256_file(packages_path)?;
    if current_sha != record.packages_sha256 {
        return Err(Error::SignatureVerification(format!(
            "Packages index for {} changed since verified update; run raptor repo update",
            entry.control.package
        )));
    }

    Ok(())
}

pub fn insecure_acquire_allowed() -> bool {
    std::env::var("RAPTOR_ALLOW_INSECURE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlFile;

    #[test]
    fn trust_record_round_trip() {
        let dir = std::env::temp_dir().join(format!("raptor-trust-{}", std::process::id()));
        let packages = dir.join("Packages");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&packages, b"Package: demo\n").unwrap();

        let record = TrustRecord {
            source_uri: "https://example.com/ubuntu".into(),
            suite: "jammy".into(),
            component: "main".into(),
            arch: "amd64".into(),
            keyring: "/etc/apt/keyrings/demo.gpg".into(),
            packages_sha256: sha256_file(&packages).unwrap(),
            verified_at_secs: 1,
        };
        write_trust_record(&packages, &record).unwrap();
        assert_eq!(load_trust_record(&packages).unwrap(), record);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_packages_trust_detects_tampering() {
        let dir = std::env::temp_dir().join(format!("raptor-trust-tamper-{}", std::process::id()));
        let packages = dir.join("Packages");
        let keyring = dir.join("keyring.gpg");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&packages, b"Package: demo\n").unwrap();
        fs::write(&keyring, b"").unwrap();

        let record = TrustRecord {
            source_uri: "https://example.com/ubuntu".into(),
            suite: "jammy".into(),
            component: "main".into(),
            arch: "amd64".into(),
            keyring: keyring.to_string_lossy().into_owned(),
            packages_sha256: sha256_file(&packages).unwrap(),
            verified_at_secs: 1,
        };
        write_trust_record(&packages, &record).unwrap();

        let entry = PackageIndexEntry {
            control: ControlFile {
                package: "demo".into(),
                ..Default::default()
            },
            file_path: packages.clone(),
            source_uri: Some(record.source_uri.clone()),
            packages_index_path: Some(packages.clone()),
            signed_by: Some(record.keyring.clone()),
            suite: Some(record.suite.clone()),
            component: Some(record.component.clone()),
        };

        verify_packages_trust(&entry).unwrap();
        fs::write(&packages, b"tampered\n").unwrap();
        assert!(verify_packages_trust(&entry).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
