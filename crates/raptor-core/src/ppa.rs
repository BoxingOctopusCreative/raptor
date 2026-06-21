use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::fs_util::move_file;
use crate::sources::{default_keyrings_dir, default_sources_list_d, SourceEntry};

/// Reference to a Launchpad PPA (`ppa:owner/repository`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpaRef {
    pub owner: String,
    pub name: String,
}

/// Resolved PPA metadata for writing sources and keyrings.
#[derive(Debug, Clone)]
pub struct PpaConfig {
    pub ppa: PpaRef,
    pub suite: String,
    pub uri: String,
    pub list_filename: String,
    pub keyring_filename: String,
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LaunchpadArchive {
  #[serde(rename = "signing_key_fingerprint")]
  signing_key_fingerprint: Option<String>,
}

/// Parse `ppa:owner/repo`, `ppa:owner/repo/ubuntu`, or `owner/repo`.
pub fn parse_ppa(input: &str) -> Result<PpaRef> {
    let input = input.trim();
    let stripped = input
        .strip_prefix("ppa:")
        .or_else(|| input.strip_prefix("PPA:"))
        .unwrap_or(input);

    let mut parts = stripped.split('/');
    let owner = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::InvalidPpa(format!("invalid PPA identifier: {input}")))?;
    let name = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::InvalidPpa(format!("invalid PPA identifier: {input}")))?;

    if !owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(Error::InvalidPpa(format!("invalid PPA owner: {owner}")));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(Error::InvalidPpa(format!("invalid PPA name: {name}")));
    }

    Ok(PpaRef {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

pub fn detect_suite() -> Result<String> {
    if let Ok(suite) = std::env::var("RAPTOR_SUITE") {
        return Ok(suite);
    }

    let os_release = Path::new("/etc/os-release");
    if os_release.exists() {
        let content = fs::read_to_string(os_release)?;
        for line in content.lines() {
            if let Some(value) = line.strip_prefix("VERSION_CODENAME=") {
                let codename = value.trim_matches('"').trim();
                if !codename.is_empty() {
                    return Ok(codename.to_string());
                }
            }
        }
        for line in content.lines() {
            if let Some(value) = line.strip_prefix("UBUNTU_CODENAME=") {
                let codename = value.trim_matches('"').trim();
                if !codename.is_empty() {
                    return Ok(codename.to_string());
                }
            }
        }
    }

    Err(Error::InvalidPpa(
        "could not detect Ubuntu suite/codename; set RAPTOR_SUITE".into(),
    ))
}

pub fn ppa_list_filename(ppa: &PpaRef, suite: &str) -> String {
    format!("{}-ubuntu-{}-{}.list", ppa.owner, ppa.name, suite)
}

pub fn ppa_keyring_filename(ppa: &PpaRef) -> String {
    format!("{}-ubuntu-{}.gpg", ppa.owner, ppa.name)
}

pub fn ppa_uri(ppa: &PpaRef) -> String {
    format!(
        "https://ppa.launchpadcontent.net/{}/{}/ubuntu",
        ppa.owner, ppa.name
    )
}

pub fn resolve_ppa(ppa: &PpaRef, suite: Option<&str>) -> Result<PpaConfig> {
    let suite = match suite {
        Some(s) => s.to_string(),
        None => detect_suite()?,
    };

    Ok(PpaConfig {
        ppa: ppa.clone(),
        suite: suite.clone(),
        uri: ppa_uri(ppa),
        list_filename: ppa_list_filename(ppa, &suite),
        keyring_filename: ppa_keyring_filename(ppa),
        signing_key_fingerprint: None,
    })
}

pub fn fetch_signing_key_fingerprint(ppa: &PpaRef) -> Result<String> {
    let url = format!(
        "https://api.launchpad.net/devel/~{}/+archive/{}",
        ppa.owner, ppa.name
    );
    let mut response = ureq::get(&url)
        .call()
        .map_err(|e| Error::PpaFetch(format!("Launchpad API request failed: {e}")))?;
    if response.status() != 200 {
        return Err(Error::PpaFetch(format!(
            "Launchpad API returned HTTP {}",
            response.status()
        )));
    }

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::PpaFetch(format!("invalid Launchpad API response: {e}")))?;
    let archive: LaunchpadArchive = serde_json::from_str(&body)
        .map_err(|e| Error::PpaFetch(format!("invalid Launchpad API response: {e}")))?;
    archive
        .signing_key_fingerprint
        .filter(|f| !f.is_empty())
        .ok_or_else(|| Error::PpaFetch("PPA has no signing key fingerprint".into()))
}

pub fn fetch_armored_key(fingerprint: &str) -> Result<String> {
    let compact = fingerprint.replace(' ', "");
    let url = format!(
        "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x{compact}"
    );
    let mut response = ureq::get(&url)
        .call()
        .map_err(|e| Error::PpaFetch(format!("keyserver request failed: {e}")))?;
    if response.status() != 200 {
        return Err(Error::PpaFetch(format!(
            "keyserver returned HTTP {}",
            response.status()
        )));
    }

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::PpaFetch(format!("invalid keyserver response: {e}")))?;
    if !body.contains("BEGIN PGP PUBLIC KEY BLOCK") {
        return Err(Error::PpaFetch(
            "keyserver did not return a PGP public key".into(),
        ));
    }
    Ok(body)
}

pub fn install_keyring(keyrings_dir: &Path, filename: &str, armored_key: &str) -> Result<PathBuf> {
    fs::create_dir_all(keyrings_dir)?;
    let asc_path = keyrings_dir.join(format!("{filename}.asc"));
    fs::write(&asc_path, armored_key)?;

    let gpg_path = keyrings_dir.join(filename);
    if Command::new("gpg")
        .args([
            "--batch",
            "--yes",
            "--dearmor",
            "--output",
            gpg_path.to_str().unwrap_or_default(),
            asc_path.to_str().unwrap_or_default(),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let _ = fs::remove_file(&asc_path);
        return Ok(gpg_path);
    }

    // Fall back to armored key if gpg is unavailable.
    let asc_dest = keyrings_dir.join(filename.replace(".gpg", ".asc"));
    if asc_dest != asc_path {
        move_file(&asc_path, &asc_dest)?;
    }
    Ok(asc_dest)
}

pub fn format_ppa_source_line(config: &PpaConfig, keyring_path: &Path) -> String {
    format!(
        "deb [signed-by={}] {} {} main\n",
        keyring_path.display(),
        config.uri,
        config.suite
    )
}

pub fn add_ppa(
    input: &str,
    suite: Option<&str>,
    sources_list_d: &Path,
    keyrings_dir: &Path,
    skip_key: bool,
) -> Result<PpaConfig> {
    let ppa = parse_ppa(input)?;
    let mut config = resolve_ppa(&ppa, suite)?;

    let list_path = sources_list_d.join(&config.list_filename);
    if list_path.exists() {
        return Err(Error::PpaExists(format!(
            "PPA already configured: ppa:{}/{}",
            ppa.owner, ppa.name
        )));
    }

    fs::create_dir_all(sources_list_d)?;

    let keyring_path = if skip_key {
        keyrings_dir.join(&config.keyring_filename)
    } else {
        let fingerprint = fetch_signing_key_fingerprint(&ppa)?;
        config.signing_key_fingerprint = Some(fingerprint.clone());
        let armored = fetch_armored_key(&fingerprint)?;
        install_keyring(keyrings_dir, &config.keyring_filename, &armored)?
    };

    let source_line = format_ppa_source_line(&config, &keyring_path);
    fs::write(&list_path, source_line)?;

    Ok(config)
}

pub fn remove_ppa(input: &str, suite: Option<&str>, sources_list_d: &Path, keyrings_dir: &Path) -> Result<()> {
    let ppa = parse_ppa(input)?;
    let config = resolve_ppa(&ppa, suite)?;
    let list_path = sources_list_d.join(&config.list_filename);

    if list_path.exists() {
        fs::remove_file(&list_path)?;
    }

    for candidate in [
        keyrings_dir.join(&config.keyring_filename),
        keyrings_dir.join(ppa_keyring_filename(&ppa).replace(".gpg", ".asc")),
    ] {
        if candidate.exists() {
            let _ = fs::remove_file(candidate);
        }
    }

    Ok(())
}

pub fn list_ppas(sources_list_d: &Path) -> Result<Vec<PpaConfig>> {
    let re = Regex::new(
        r"deb\s+\[signed-by=[^\]]+\]\s+https://ppa\.launchpadcontent\.net/([^/]+)/([^/]+)/ubuntu\s+(\S+)\s+main",
    )
    .unwrap();
    let legacy = Regex::new(
        r"deb\s+https?://ppa\.launchpad(?:content)?\.net/([^/]+)/([^/]+)/ubuntu\s+(\S+)\s+main",
    )
    .unwrap();

    let mut ppas = Vec::new();
    if !sources_list_d.is_dir() {
        return Ok(ppas);
    }

    for entry in fs::read_dir(sources_list_d)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "list") {
            let content = fs::read_to_string(entry.path())?;
            for line in content.lines() {
                if let Some(caps) = re.captures(line).or_else(|| legacy.captures(line)) {
                    let owner = caps[1].to_string();
                    let name = caps[2].to_string();
                    let suite = caps[3].to_string();
                    let ppa = PpaRef { owner, name };
                    ppas.push(PpaConfig {
                        ppa: ppa.clone(),
                        suite: suite.clone(),
                        uri: ppa_uri(&ppa),
                        list_filename: ppa_list_filename(&ppa, &suite),
                        keyring_filename: ppa_keyring_filename(&ppa),
                        signing_key_fingerprint: None,
                    });
                }
            }
        }
    }

    ppas.sort_by(|a, b| {
        (&a.ppa.owner, &a.ppa.name, &a.suite).cmp(&(&b.ppa.owner, &b.ppa.name, &b.suite))
    });
    Ok(ppas)
}

pub fn is_ppa_source(entry: &SourceEntry) -> bool {
    entry.uri.contains("ppa.launchpadcontent.net") || entry.uri.contains("ppa.launchpad.net")
}

pub fn sources_list_d_path() -> PathBuf {
    std::env::var("RAPTOR_SOURCES_LIST_D")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_sources_list_d())
}

pub fn keyrings_dir_path() -> PathBuf {
    std::env::var("RAPTOR_KEYRINGS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_keyrings_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ppa_identifiers() {
        let ppa = parse_ppa("ppa:git-core/cargo").unwrap();
        assert_eq!(ppa.owner, "git-core");
        assert_eq!(ppa.name, "cargo");

        let ppa = parse_ppa("git-core/cargo").unwrap();
        assert_eq!(ppa.owner, "git-core");
        assert_eq!(ppa.name, "cargo");
    }

    #[test]
    fn builds_launchpad_uri() {
        let ppa = parse_ppa("ppa:git-core/cargo").unwrap();
        assert_eq!(
            ppa_uri(&ppa),
            "https://ppa.launchpadcontent.net/git-core/cargo/ubuntu"
        );
    }
}
