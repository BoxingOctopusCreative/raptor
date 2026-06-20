use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct SourceEntry {
    pub source_type: SourceType,
    pub uri: String,
    pub suite: String,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
    pub enabled: bool,
    pub signed_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Deb,
    DebSrc,
}

#[derive(Debug, Default)]
pub struct SourcesList {
    pub entries: Vec<SourceEntry>,
}

impl SourcesList {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (source_type, enabled, rest) = if let Some(rest) = line.strip_prefix("deb-src ") {
                (SourceType::DebSrc, true, rest)
            } else if let Some(rest) = line.strip_prefix("# deb-src ") {
                (SourceType::DebSrc, false, rest)
            } else if let Some(rest) = line.strip_prefix("deb ") {
                (SourceType::Deb, true, rest)
            } else if let Some(rest) = line.strip_prefix("# deb ") {
                (SourceType::Deb, false, rest)
            } else {
                continue;
            };

            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                return Err(Error::InvalidSources(format!("invalid source line: {rest}")));
            }

            let (signed_by, parts) = parse_source_options(parts)?;
            if parts.len() < 3 {
                return Err(Error::InvalidSources(format!("invalid source line: {rest}")));
            }

            let uri = parts[0].to_string();
            let suite = parts[1].to_string();
            let components = parts[2..].iter().map(|s| s.to_string()).collect();

            entries.push(SourceEntry {
                source_type,
                uri,
                suite,
                components,
                architectures: Vec::new(),
                enabled,
                signed_by,
            });
        }
        Ok(Self { entries })
    }

    pub fn package_index_paths(&self, cache_dir: &Path, arch: &str) -> Vec<PathBuf> {
        let deb_arch = crate::state::deb_architecture(arch);
        let mut paths = Vec::new();
        for entry in &self.entries {
            if !entry.enabled || entry.source_type != SourceType::Deb {
                continue;
            }
            for component in &entry.components {
                if entry.uri.starts_with("http://") || entry.uri.starts_with("https://") {
                    let local = cache_dir.join(url_to_cache_name(&entry.uri)).join(format!(
                        "dists/{}/{}/binary-{}/Packages",
                        entry.suite, component, deb_arch
                    ));
                    paths.push(local);
                    continue;
                }
                let local = cache_dir.join(url_to_cache_name(&entry.uri)).join(format!(
                    "dists/{}/{}/binary-{}/Packages",
                    entry.suite, component, deb_arch
                ));
                paths.push(local.clone());
                paths.push(local.with_extension("gz"));
            }
        }
        paths
    }

    pub fn local_repo_roots(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .filter(|e| e.enabled && e.source_type == SourceType::Deb)
            .filter_map(|e| {
                if e.uri.starts_with("file:") {
                    Some(PathBuf::from(e.uri.trim_start_matches("file:")))
                } else if e.uri.starts_with('/') {
                    Some(PathBuf::from(&e.uri))
                } else {
                    None
                }
            })
            .collect()
    }
}

fn url_to_cache_name(uri: &str) -> String {
    uri.trim_start_matches("file:")
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .replace('/', "_")
}

pub fn default_sources_path() -> PathBuf {
    PathBuf::from("/etc/apt/sources.list")
}

pub fn default_sources_list_d() -> PathBuf {
    PathBuf::from("/etc/apt/sources.list.d")
}

pub fn default_keyrings_dir() -> PathBuf {
    PathBuf::from("/etc/apt/keyrings")
}

fn parse_source_options(parts: Vec<&str>) -> Result<(Option<String>, Vec<&str>)> {
    if let Some(first) = parts.first() {
        if first.starts_with('[') && first.ends_with(']') {
            let options = &first[1..first.len() - 1];
            let signed_by = options
                .split_whitespace()
                .find_map(|opt| opt.strip_prefix("signed-by="))
                .map(str::to_string);
            return Ok((signed_by, parts[1..].to_vec()));
        }
    }
    Ok((None, parts))
}

pub fn load_all_sources() -> Result<SourcesList> {
    if let Ok(path) = std::env::var("RAPTOR_SOURCES") {
        return SourcesList::load(Path::new(&path));
    }

    let mut all = SourcesList::default();
    let main = default_sources_path();
    if main.exists() {
        all.entries.extend(SourcesList::load(&main)?.entries);
    }
    let list_d = default_sources_list_d();
    if list_d.is_dir() {
        for entry in fs::read_dir(list_d)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "list") {
                all.entries.extend(SourcesList::load(&path)?.entries);
            }
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signed_by_sources() {
        let content = "deb [signed-by=/etc/apt/keyrings/example.gpg] https://ppa.launchpadcontent.net/user/repo/ubuntu jammy main\n";
        let sources = SourcesList::parse(content).unwrap();
        assert_eq!(sources.entries.len(), 1);
        assert_eq!(
            sources.entries[0].signed_by.as_deref(),
            Some("/etc/apt/keyrings/example.gpg")
        );
        assert_eq!(sources.entries[0].uri, "https://ppa.launchpadcontent.net/user/repo/ubuntu");
        assert_eq!(sources.entries[0].suite, "jammy");
    }
}
