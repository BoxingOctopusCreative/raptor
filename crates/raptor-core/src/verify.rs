use std::fs;
use std::path::Path;
use std::process::Command;

use md5::{Digest as Md5Digest, Md5};
use sha2::Sha256;

use crate::error::{Error, Result};
use crate::release::{extract_inrelease_body, ReleaseChecksum, ReleaseIndex};

/// Verify a clearsigned file (e.g. `InRelease`) and return the signed Release body.
pub fn verify_and_extract_inrelease(keyring: &Path, inrelease_path: &Path) -> Result<String> {
    verify_clearsigned(keyring, inrelease_path)?;
    let content = fs::read_to_string(inrelease_path)?;
    extract_inrelease_body(&content)
}

/// Verify a detached OpenPGP signature over arbitrary payload bytes.
pub fn verify_detached_signature(
    keyring: &Path,
    signature_path: &Path,
    signed_path: &Path,
) -> Result<()> {
    verify_detached(keyring, signature_path, signed_path)
}

/// Verify a detached signature over a `Release` file.
pub fn verify_detached_release(
    keyring: &Path,
    release_path: &Path,
    signature_path: &Path,
) -> Result<()> {
    verify_detached(keyring, signature_path, release_path)
}

/// Verify downloaded file bytes against checksums declared in `Release`.
pub fn verify_payload_checksums(path: &Path, expected: &ReleaseChecksum) -> Result<()> {
    let bytes = fs::read(path)?;
    if bytes.len() as u64 != expected.size {
        return Err(Error::ChecksumMismatch(format!(
            "size mismatch for {}: expected {}, got {}",
            path.display(),
            expected.size,
            bytes.len()
        )));
    }

    if let Some(expected_sha) = &expected.sha256 {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != *expected_sha {
            return Err(Error::ChecksumMismatch(format!(
                "SHA256 mismatch for {}",
                path.display()
            )));
        }
        return Ok(());
    }

    if let Some(expected_md5) = &expected.md5 {
        let actual = format!("{:x}", Md5::digest(&bytes));
        if actual != *expected_md5 {
            return Err(Error::ChecksumMismatch(format!(
                "MD5 mismatch for {}",
                path.display()
            )));
        }
        return Ok(());
    }

    Err(Error::ChecksumMismatch(format!(
        "Release entry for {} has no supported checksum",
        path.display()
    )))
}

pub fn parse_release_file(path: &Path) -> Result<ReleaseIndex> {
    let content = fs::read_to_string(path)?;
    ReleaseIndex::parse(&content)
}

/// After download, verify `.deb` authenticity via detached signature or `debsig-verify`.
///
/// Standard Debian pool packages are trusted through the signed `Release` → `Packages` chain
/// (validated separately). This step adds optional per-package signatures when present.
pub fn verify_deb_package_signature(
    keyring: &Path,
    deb_path: &Path,
    detached_sig_path: Option<&Path>,
) -> Result<()> {
    if let Some(sig_path) = detached_sig_path {
        verify_detached_signature(keyring, sig_path, deb_path)?;
        return Ok(());
    }

    if try_debsig_verify(deb_path)? {
        return Ok(());
    }

    Ok(())
}

fn try_debsig_verify(deb_path: &Path) -> Result<bool> {
    if std::env::var("RAPTOR_DEBSIG_VERIFY")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        return Ok(false);
    }

    if !has_command("debsig-verify") {
        return Ok(false);
    }

    let status = Command::new("debsig-verify")
        .arg("--verify")
        .arg(deb_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::SignatureVerification(format!("failed to run debsig-verify: {e}")))?;

    if status.success() {
        Ok(true)
    } else {
        Err(Error::SignatureVerification(format!(
            "debsig-verify rejected {}",
            deb_path.display()
        )))
    }
}

fn verify_clearsigned(keyring: &Path, clearsigned_path: &Path) -> Result<()> {
    if run_gpgv(keyring, &[clearsigned_path])? {
        return Ok(());
    }
    run_gpg_verify(keyring, &["--verify", clearsigned_path.to_str().unwrap_or_default()])
}

fn verify_detached(keyring: &Path, signature_path: &Path, signed_path: &Path) -> Result<()> {
    if run_gpgv(
        keyring,
        &[signature_path, signed_path],
    )? {
        return Ok(());
    }
    run_gpg_verify(
        keyring,
        &[
            "--verify",
            signature_path.to_str().unwrap_or_default(),
            signed_path.to_str().unwrap_or_default(),
        ],
    )
}

fn run_gpgv(keyring: &Path, paths: &[&Path]) -> Result<bool> {
    if !has_command("gpgv") {
        return Ok(false);
    }
    if !keyring.exists() {
        return Err(Error::SignatureVerification(format!(
            "keyring not found: {}",
            keyring.display()
        )));
    }

    let mut cmd = Command::new("gpgv");
    cmd.arg("--keyring").arg(keyring);
    for path in paths {
        cmd.arg(path);
    }

    let status = cmd
        .status()
        .map_err(|e| Error::SignatureVerification(format!("failed to run gpgv: {e}")))?;
    if status.success() {
        Ok(true)
    } else {
        Err(Error::SignatureVerification(
            "gpgv rejected repository signature".into(),
        ))
    }
}

fn run_gpg_verify(keyring: &Path, gpg_args: &[&str]) -> Result<()> {
    if !has_command("gpg") {
        return Err(Error::SignatureVerification(
            "neither gpgv nor gpg is available for signature verification".into(),
        ));
    }
    if !keyring.exists() {
        return Err(Error::SignatureVerification(format!(
            "keyring not found: {}",
            keyring.display()
        )));
    }

    let status = Command::new("gpg")
        .arg("--batch")
        .arg("--no-default-keyring")
        .arg("--keyring")
        .arg(keyring)
        .args(gpg_args)
        .status()
        .map_err(|e| Error::SignatureVerification(format!("failed to run gpg: {e}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::SignatureVerification(
            "gpg rejected repository signature".into(),
        ))
    }
}

fn has_command(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
