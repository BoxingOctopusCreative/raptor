use std::fs;
use std::path::{Path, PathBuf};

use md5::{Digest as Md5Digest, Md5};
use sha2::Sha256;

use crate::control::ControlFile;
use crate::deb::read_deb;
use crate::error::{Error, Result};
use crate::fs_util::move_file;
use crate::remote::{download_bytes, download_bytes_near};
use crate::repository::PackageIndexEntry;
use crate::trust::{insecure_acquire_allowed, verify_packages_trust};
use crate::verify::verify_deb_package_signature;

#[derive(Debug, Clone)]
pub struct AcquireContext {
    pub archives_dir: PathBuf,
}

/// Repository pin priority for `.deb` files passed directly to `raptor pkg get`.
pub const DIRECT_DEB_PRIORITY: i32 = i32::MAX;

#[derive(Debug, Clone)]
pub struct DirectDeb {
    pub path: PathBuf,
    pub remote_spec: Option<String>,
}

/// Whether `spec` refers to a local `.deb` path or remote `.deb` URL (not a repository package name).
pub fn is_deb_spec(spec: &str) -> bool {
    if spec.starts_with("http://") || spec.starts_with("https://") || spec.starts_with("file://") {
        return spec.ends_with(".deb");
    }
    Path::new(spec)
        .extension()
        .is_some_and(|ext| ext == "deb")
}

/// Copy or download an arbitrary `.deb` into the archives cache.
pub fn acquire_direct_deb(spec: &str, ctx: &AcquireContext) -> Result<DirectDeb> {
    if !is_deb_spec(spec) {
        return Err(Error::PackageAcquire(format!("not a .deb path or URL: {spec}")));
    }

    fs::create_dir_all(&ctx.archives_dir)?;

    let remote_spec = if spec.starts_with("http://") || spec.starts_with("https://") {
        Some(spec.to_string())
    } else {
        None
    };

    let local = if let Some(url) = &remote_spec {
        download_bytes_near(url, &ctx.archives_dir).map_err(|e| {
            Error::PackageAcquire(format!("failed to download {url}: {e}"))
        })?
    } else {
        let path = if let Some(local) = spec.strip_prefix("file://") {
            PathBuf::from(local)
        } else {
            PathBuf::from(spec)
        };
        if !path.is_file() {
            return Err(Error::PackageAcquire(format!(
                "cannot access archive '{spec}': No such file or directory"
            )));
        }
        path
    };

    let mut deb = read_deb(&local)?;
    deb.control = enrich_direct_deb_control(deb.control, spec);
    let dest = ctx.archives_dir.join(deb.control.full_name());

    if local != dest {
        if dest.exists() {
            fs::remove_file(&dest)?;
        }
        if remote_spec.is_some() {
            move_file(&local, &dest)?;
        } else {
            fs::copy(&local, &dest)?;
        }
    }

    Ok(DirectDeb {
        path: dest,
        remote_spec,
    })
}

/// Fill in missing control fields for a directly requested `.deb`.
pub fn enrich_direct_deb_control(mut control: ControlFile, spec: &str) -> ControlFile {
    if control.architecture.is_empty() {
        if let Some(arch) = infer_arch_from_deb_spec(spec) {
            control.architecture = arch;
        }
    }
    control
}

fn infer_arch_from_deb_spec(spec: &str) -> Option<String> {
    let name = spec.rsplit('/').next()?.strip_suffix(".deb")?;
    let arch = name.rsplit('_').next()?;
    if is_known_deb_arch(arch) {
        Some(arch.to_string())
    } else {
        None
    }
}

fn is_known_deb_arch(arch: &str) -> bool {
    matches!(
        arch,
        "all" | "amd64"
            | "arm64"
            | "armhf"
            | "i386"
            | "ppc64el"
            | "riscv64"
            | "s390x"
    )
}

/// Build an index entry for a locally acquired `.deb` (wins over repository versions).
pub fn local_deb_index_entry(deb_path: PathBuf, control: ControlFile) -> PackageIndexEntry {
    PackageIndexEntry {
        control,
        file_path: deb_path,
        source_uri: None,
        packages_index_path: None,
        signed_by: None,
        suite: None,
        component: None,
        repo_priority: DIRECT_DEB_PRIORITY,
    }
}

/// Ensure a `.deb` is available locally, downloading from the configured repository if needed.
pub fn ensure_deb(entry: &PackageIndexEntry, ctx: &AcquireContext) -> Result<PathBuf> {
    if entry.file_path.exists() {
        verify_control_checksums(&entry.file_path, &entry.control)?;
        return Ok(entry.file_path.clone());
    }

    let source_uri = entry.source_uri.as_ref().ok_or_else(|| {
        Error::PackageAcquire(format!(
            "package {} is not available locally; run raptor repo update",
            entry.control.package
        ))
    })?;

    let filename = entry.control.filename.trim();
    if filename.is_empty() {
        return Err(Error::PackageAcquire(format!(
            "package {} has no Filename in repository index",
            entry.control.package
        )));
    }

    let requires_trust = entry.signed_by.is_some() && !insecure_acquire_allowed();
    if requires_trust {
        verify_packages_trust(entry)?;
    }

    fs::create_dir_all(&ctx.archives_dir)?;
    let dest = ctx.archives_dir.join(entry.control.full_name());
    if dest.exists() {
        match verify_control_checksums(&dest, &entry.control) {
            Ok(()) => {
                verify_deb_gpg_if_required(entry, source_uri, filename, &dest)?;
                return Ok(dest);
            }
            Err(_) => {
                let _ = fs::remove_file(&dest);
            }
        }
    }

    let url = build_package_url(source_uri, filename);
    let temp = download_bytes_near(&url, &ctx.archives_dir).map_err(|e| {
        Error::PackageAcquire(format!(
            "failed to download {}: {e}",
            entry.control.package
        ))
    })?;

    verify_control_checksums(&temp, &entry.control).map_err(|e| {
        let _ = fs::remove_file(&temp);
        e
    })?;

    verify_deb_gpg_if_required(entry, source_uri, filename, &temp)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    move_file(&temp, &dest)?;

    Ok(dest)
}

fn verify_deb_gpg_if_required(
    entry: &PackageIndexEntry,
    source_uri: &str,
    filename: &str,
    deb_path: &Path,
) -> Result<()> {
    let Some(keyring_path) = entry.signed_by.as_deref() else {
        return Ok(());
    };

    let keyring = Path::new(keyring_path);
    let deb_url = build_package_url(source_uri, filename);
    let sig_url = format!("{deb_url}.gpg");
    let detached_sig = download_bytes(&sig_url).ok();
    let result = verify_deb_package_signature(keyring, deb_path, detached_sig.as_deref());
    if let Some(sig) = detached_sig {
        let _ = fs::remove_file(sig);
    }
    result
}

/// Build the HTTP(S) URL for a pool package from repository base URI and `Filename` field.
pub fn build_package_url(base_uri: &str, filename: &str) -> String {
    let base = base_uri.trim_end_matches('/');
    let file = filename.trim_start_matches('/');
    format!("{base}/{file}")
}

pub fn verify_control_checksums(path: &Path, control: &ControlFile) -> Result<()> {
    let bytes = fs::read(path)?;

    if !control.size.is_empty() {
        let expected: u64 = control.size.parse().map_err(|_| {
            Error::ChecksumMismatch(format!("invalid Size field for {}", control.package))
        })?;
        if bytes.len() as u64 != expected {
            return Err(Error::ChecksumMismatch(format!(
                "size mismatch for {}: expected {}, got {}",
                control.package,
                expected,
                bytes.len()
            )));
        }
    }

    if !control.sha256.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != control.sha256.to_ascii_lowercase() {
            return Err(Error::ChecksumMismatch(format!(
                "SHA256 mismatch for {}",
                control.package
            )));
        }
        return Ok(());
    }

    if !control.md5sum.is_empty() {
        let actual = format!("{:x}", Md5::digest(&bytes));
        if actual != control.md5sum.to_ascii_lowercase() {
            return Err(Error::ChecksumMismatch(format!(
                "MD5 mismatch for {}",
                control.package
            )));
        }
        return Ok(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_deb_specs() {
        assert!(is_deb_spec("./hello_1.0_all.deb"));
        assert!(is_deb_spec("/tmp/pkg_2.0_amd64.deb"));
        assert!(is_deb_spec("file:///tmp/pkg_2.0_amd64.deb"));
        assert!(is_deb_spec("https://example.com/pool/pkg_1.0_amd64.deb"));
        assert!(is_deb_spec("raptor_0.6.0_amd64.deb"));
        assert!(!is_deb_spec("hello-raptor"));
        assert!(!is_deb_spec("https://example.com/ubuntu/dists/stable/Release"));
    }

    #[test]
    fn infers_architecture_from_deb_filename() {
        let control = enrich_direct_deb_control(
            ControlFile {
                package: "raptor".into(),
                version: "0.6.0".into(),
                ..Default::default()
            },
            "raptor_0.6.0_amd64.deb",
        );
        assert_eq!(control.architecture, "amd64");
    }

    #[test]
    fn builds_pool_download_url() {
        let url = build_package_url(
            "https://ppa.launchpadcontent.net/git-core/cargo/ubuntu",
            "pool/main/c/cargo_1.0_amd64.deb",
        );
        assert_eq!(
            url,
            "https://ppa.launchpadcontent.net/git-core/cargo/ubuntu/pool/main/c/cargo_1.0_amd64.deb"
        );
    }
}
