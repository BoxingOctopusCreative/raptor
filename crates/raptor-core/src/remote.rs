use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

use crate::error::{Error, Result};
use crate::fs_util::{move_file, temp_file_in};
use crate::release::ReleaseIndex;
use crate::sources::{SourceEntry, SourceType, SourcesList};
use crate::state::deb_architecture;
use crate::trust::{now_secs, sha256_file, write_trust_record, TrustRecord};
use crate::verify::{
    parse_release_file, verify_and_extract_inrelease, verify_detached_release,
    verify_payload_checksums,
};

pub fn fetch_remote_indexes(
    sources: &SourcesList,
    cache_dir: &Path,
    arch: &str,
) -> Result<Vec<(String, PathBuf)>> {
    let deb_arch = deb_architecture(arch);
    let mut fetched = Vec::new();
    let allow_insecure = insecure_updates_allowed();

    for entry in &sources.entries {
        if !entry.enabled || entry.source_type != SourceType::Deb {
            continue;
        }
        if !is_remote(&entry.uri) {
            continue;
        }

        let keyring = entry.signed_by.as_deref().map(Path::new);
        if keyring.is_none() && !allow_insecure {
            return Err(Error::InsecureRepository(format!(
                "remote source {} has no signed-by keyring; set signed-by or RAPTOR_ALLOW_INSECURE=1",
                entry.uri
            )));
        }

        let release_index: Option<ReleaseIndex> = if let Some(keyring) = keyring {
            Some(fetch_verified_release_index(entry, keyring)?)
        } else {
            eprintln!(
                "W: updating {} without signature verification (RAPTOR_ALLOW_INSECURE=1)",
                entry.uri
            );
            None
        };

        for component in &entry.components {
            let rel_paths = [
                format!("{component}/binary-{deb_arch}/Packages.gz"),
                format!("{component}/binary-{deb_arch}/Packages"),
            ];
            let cache_base = cache_dir
                .join(url_to_cache_name(&entry.uri))
                .join(format!(
                    "dists/{}/{}/binary-{}/",
                    entry.suite, component, deb_arch
                ));
            fs::create_dir_all(&cache_base)?;

            let mut downloaded = false;
            for rel_path in &rel_paths {
                let suite_base = format!(
                    "{}/dists/{}/",
                    entry.uri.trim_end_matches('/'),
                    entry.suite
                );
                let url = format!("{suite_base}{rel_path}");

                let local_path = if let Some(index) = &release_index {
                    let Some(checksum) = index.checksum(rel_path) else {
                        continue;
                    };
                    let temp = download_bytes_near(&url, &cache_base)?;
                    verify_payload_checksums(&temp, checksum)?;
                    if rel_path.ends_with(".gz") {
                        let plain = cache_base.join("Packages");
                        decompress_to_file(&temp, &plain)?;
                        let _ = fs::remove_file(&temp);
                        plain
                    } else {
                        let plain = cache_base.join("Packages");
                        move_file(&temp, &plain)?;
                        plain
                    }
                } else if let Some((_, temp)) =
                    try_download_first(&[url.clone()], &cache_base).ok().flatten()
                {
                    if rel_path.ends_with(".gz") {
                        let plain = cache_base.join("Packages");
                        decompress_to_file(&temp, &plain)?;
                        let _ = fs::remove_file(&temp);
                        plain
                    } else {
                        let plain = cache_base.join("Packages");
                        move_file(&temp, &plain)?;
                        plain
                    }
                } else {
                    continue;
                };

                fetched.push((url, local_path.clone()));
                downloaded = true;

                if let Some(keyring) = keyring {
                    let record = TrustRecord {
                        source_uri: entry.uri.clone(),
                        suite: entry.suite.clone(),
                        component: component.clone(),
                        arch: deb_arch.to_string(),
                        keyring: keyring.to_string_lossy().into_owned(),
                        packages_sha256: sha256_file(&local_path)?,
                        verified_at_secs: now_secs(),
                    };
                    write_trust_record(&local_path, &record)?;
                }

                break;
            }

            if !downloaded && release_index.is_some() {
                eprintln!(
                    "W: no Packages index listed in Release for {}/{}/binary-{}",
                    entry.suite, component, deb_arch
                );
            }
        }
    }

    Ok(fetched)
}

fn fetch_verified_release_index(entry: &SourceEntry, keyring: &Path) -> Result<ReleaseIndex> {
    let suite_base = format!(
        "{}/dists/{}/",
        entry.uri.trim_end_matches('/'),
        entry.suite
    );

    let inrelease_url = format!("{suite_base}InRelease");
    if let Ok(temp) = download_bytes(&inrelease_url) {
        match verify_and_extract_inrelease(keyring, &temp) {
            Ok(body) => {
                let _ = fs::remove_file(&temp);
                return ReleaseIndex::parse(&body);
            }
            Err(e) => {
                let _ = fs::remove_file(&temp);
                eprintln!("W: InRelease verification failed for {}: {e}", entry.uri);
            }
        }
    }

    let release_url = format!("{suite_base}Release");
    let release_gpg_url = format!("{suite_base}Release.gpg");
    let release_temp = download_bytes(&release_url).map_err(|e| {
        Error::SignatureVerification(format!(
            "could not fetch signed Release for {}: {e}",
            entry.uri
        ))
    })?;
    let sig_temp = download_bytes(&release_gpg_url).map_err(|e| {
        let _ = fs::remove_file(&release_temp);
        Error::SignatureVerification(format!(
            "could not fetch Release.gpg for {}: {e}",
            entry.uri
        ))
    })?;

    verify_detached_release(keyring, &release_temp, &sig_temp)?;
    let index = parse_release_file(&release_temp)?;
    let _ = fs::remove_file(&release_temp);
    let _ = fs::remove_file(&sig_temp);
    Ok(index)
}

fn insecure_updates_allowed() -> bool {
    std::env::var("RAPTOR_ALLOW_INSECURE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn is_remote(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

fn url_to_cache_name(uri: &str) -> String {
    uri.trim_start_matches("https://")
        .trim_start_matches("http://")
        .replace('/', "_")
}

fn try_download_first(urls: &[String], dir: &Path) -> Result<Option<(String, PathBuf)>> {
    for url in urls {
        match download_bytes_near(url, dir) {
            Ok(path) => return Ok(Some((url.clone(), path))),
            Err(e) => eprintln!("W: {e}"),
        }
    }
    Ok(None)
}

pub fn download_bytes(url: &str) -> Result<PathBuf> {
    fetch_url(url)
}

pub fn download_bytes_near(url: &str, dir: &Path) -> Result<PathBuf> {
    fetch_url_near(url, dir)
}

/// Fetch bytes from `http(s)://` or `file://` URLs (used by mirror sync tests with local mocks).
pub fn fetch_url(url: &str) -> Result<PathBuf> {
    fetch_url_near(url, std::env::temp_dir().as_path())
}

/// Fetch bytes into a temp file under `dir` so final moves stay on one filesystem.
pub fn fetch_url_near(url: &str, dir: &Path) -> Result<PathBuf> {
    if let Some(local) = url.strip_prefix("file://") {
        return copy_local_to_temp_near(Path::new(local), dir);
    }
    if url.starts_with('/') {
        return copy_local_to_temp_near(Path::new(url), dir);
    }

    let mut response = ureq::get(url)
        .call()
        .map_err(|e| Error::RemoteFetch(format!("GET {url}: {e}")))?;
    if response.status() != 200 {
        return Err(Error::RemoteFetch(format!(
            "GET {url}: HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| Error::RemoteFetch(e.to_string()))?;

    write_temp_bytes_near(&bytes, dir)
}

fn copy_local_to_temp_near(path: &Path, dir: &Path) -> Result<PathBuf> {
    let bytes = fs::read(path).map_err(|e| {
        Error::RemoteFetch(format!("read {}: {e}", path.display()))
    })?;
    write_temp_bytes_near(&bytes, dir)
}

fn write_temp_bytes_near(bytes: &[u8], dir: &Path) -> Result<PathBuf> {
    let temp = temp_file_in(dir, "fetch")?;
    fs::write(&temp, bytes)?;
    Ok(temp)
}

fn decompress_to_file(gz_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(gz_path)?;
    let mut decoder = GzDecoder::new(file);
    let mut content = String::new();
    decoder
        .read_to_string(&mut content)
        .map_err(|e| Error::RemoteFetch(e.to_string()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest, content)?;
    Ok(())
}

pub fn remote_package_index_paths(sources: &SourcesList, cache_dir: &Path, arch: &str) -> Vec<PathBuf> {
    let deb_arch = deb_architecture(arch);
    let mut paths = Vec::new();
    for entry in &sources.entries {
        if !entry.enabled || entry.source_type != SourceType::Deb || !is_remote(&entry.uri) {
            continue;
        }
        for component in &entry.components {
            let local = cache_dir.join(url_to_cache_name(&entry.uri)).join(format!(
                "dists/{}/{}/binary-{}/Packages",
                entry.suite, component, deb_arch
            ));
            paths.push(local);
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insecure_flag_parsing() {
        std::env::set_var("RAPTOR_ALLOW_INSECURE", "1");
        assert!(insecure_updates_allowed());
        std::env::remove_var("RAPTOR_ALLOW_INSECURE");
        assert!(!insecure_updates_allowed());
    }

    #[test]
    fn fetch_url_reads_local_file() {
        let path = std::env::temp_dir().join(format!("raptor-fetch-file-{}", std::process::id()));
        fs::write(&path, b"mock payload").unwrap();
        let url = format!("file://{}", path.display());
        let temp = fetch_url(&url).unwrap();
        let content = fs::read_to_string(&temp).unwrap();
        assert_eq!(content, "mock payload");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&temp);
    }
}
