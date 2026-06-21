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
    /// Repository pin priority (higher wins when package versions tie).
    pub priority: i32,
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
        if is_deb822_sources_path(path) || looks_like_deb822(&content) {
            Self::parse_deb822(&content)
        } else {
            Self::parse(&content)
        }
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
                priority: 500,
            });
        }
        Ok(Self { entries })
    }

    /// Parse APT deb822-format `.sources` files (Ubuntu 24.04+ default).
    pub fn parse_deb822(content: &str) -> Result<Self> {
        let mut entries = Vec::new();

        for stanza in parse_deb822_stanzas(content) {
            let enabled = stanza
                .get("enabled")
                .map(|values| {
                    !values
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case("no"))
                })
                .unwrap_or(true);
            if !enabled {
                continue;
            }

            let types = deb822_source_types(stanza.get("types"));
            let uris = deb822_field_values(stanza.get("uris"));
            let suites = deb822_field_values(stanza.get("suites").or(stanza.get("suite")));
            let components = deb822_field_values(stanza.get("components"));
            let architectures = deb822_field_values(stanza.get("architectures"));
            let signed_by = stanza
                .get("signed-by")
                .and_then(|values| values.first())
                .cloned();

            if uris.is_empty() || suites.is_empty() {
                return Err(Error::InvalidSources(
                    "deb822 stanza missing URIs or Suites".into(),
                ));
            }

            for source_type in types {
                for uri in &uris {
                    for suite in &suites {
                        entries.push(SourceEntry {
                            source_type,
                            uri: uri.clone(),
                            suite: suite.clone(),
                            components: components.clone(),
                            architectures: architectures.clone(),
                            enabled: true,
                            signed_by: signed_by.clone(),
                            priority: 500,
                        });
                    }
                }
            }
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

pub fn is_apt_sources_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "list" || ext == "sources")
}

fn is_deb822_sources_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "sources")
}

fn looks_like_deb822(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .is_some_and(|line| line.contains(':') && !line.starts_with("deb "))
}

fn parse_deb822_stanzas(content: &str) -> Vec<std::collections::HashMap<String, Vec<String>>> {
    use std::collections::HashMap;

    let mut stanzas = Vec::new();
    let mut current: HashMap<String, Vec<String>> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                stanzas.push(current);
                current = HashMap::new();
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            current
                .entry(key.trim().to_ascii_lowercase())
                .or_default()
                .extend(value.split_whitespace().map(str::to_string));
        }
    }

    if !current.is_empty() {
        stanzas.push(current);
    }

    stanzas
}

fn deb822_field_values(values: Option<&Vec<String>>) -> Vec<String> {
    values
        .map(|values| {
            values
                .iter()
                .flat_map(|value| value.split_whitespace().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn deb822_source_types(values: Option<&Vec<String>>) -> Vec<SourceType> {
    let mut types = Vec::new();
    for value in deb822_field_values(values) {
        match value.as_str() {
            "deb" => types.push(SourceType::Deb),
            "deb-src" => types.push(SourceType::DebSrc),
            _ => {}
        }
    }
    if types.is_empty() {
        types.push(SourceType::Deb);
    }
    types
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

/// Merge APT `sources.list` and `sources.list.d` entries not already present in `all`.
///
/// When `/etc/raptor/sources.d` YAML is incomplete (e.g. only `main`), Ubuntu `.sources`
/// stanzas still supply `universe` and other components.
fn merge_apt_source_extras(all: &mut SourcesList, main: &Path, list_d: &Path) -> Result<()> {
    use std::collections::HashSet;

    if !main.exists() && (!list_d.is_dir() || fs::read_dir(list_d)?.next().is_none()) {
        return Ok(());
    }

    let mut existing: HashSet<(String, String, String)> = all
        .entries
        .iter()
        .map(|entry| {
            (
                entry.uri.clone(),
                entry.suite.clone(),
                entry.components.join(","),
            )
        })
        .collect();

    let mut extras = SourcesList::default();
    if main.exists() {
        extras.entries.extend(SourcesList::load(main)?.entries);
    }
    if list_d.is_dir() {
        let mut list_paths: Vec<PathBuf> = fs::read_dir(list_d)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| is_apt_sources_file(path))
            .collect();
        list_paths.sort();
        for path in list_paths {
            extras.entries.extend(SourcesList::load(&path)?.entries);
        }
    }

    for entry in extras.entries {
        let key = (
            entry.uri.clone(),
            entry.suite.clone(),
            entry.components.join(","),
        );
        if existing.insert(key) {
            all.entries.push(entry);
        }
    }

    Ok(())
}

pub fn load_all_sources() -> Result<SourcesList> {
    if let Ok(path) = std::env::var("RAPTOR_SOURCES_D") {
        let path = Path::new(&path);
        if path.is_dir() {
            let mut list = crate::sources_config::load_sources_from_dir(path)?;
            let list_d = std::env::var("RAPTOR_SOURCES_LIST_D")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_sources_list_d());
            let main = std::env::var("RAPTOR_SOURCES")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_sources_path());
            merge_apt_source_extras(&mut list, &main, &list_d)?;
            return Ok(list);
        }
    }

    if let Ok(config) = crate::config::RaptorConfig::load() {
        if config.paths.sources_d.is_dir() {
            let mut list = crate::sources_config::load_sources_from_dir(&config.paths.sources_d)?;
            if !list.entries.is_empty() {
                merge_apt_source_extras(&mut list, &config.paths.sources, &config.paths.sources_list_d)?;
                return Ok(list);
            }
        }
    }

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
            if is_apt_sources_file(&path) {
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

    #[test]
    fn parses_deb822_sources() {
        let content = r#"Types: deb
URIs: http://archive.ubuntu.com/ubuntu
Suites: resolute resolute-updates
Components: main universe
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg

Types: deb
URIs: http://security.ubuntu.com/ubuntu
Suites: resolute-security
Components: main
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
"#;
        let sources = SourcesList::parse_deb822(content).unwrap();
        assert_eq!(sources.entries.len(), 3);
        assert_eq!(sources.entries[0].uri, "http://archive.ubuntu.com/ubuntu");
        assert_eq!(sources.entries[0].suite, "resolute");
        assert_eq!(sources.entries[0].components, vec!["main", "universe"]);
        assert_eq!(
            sources.entries[0].signed_by.as_deref(),
            Some("/usr/share/keyrings/ubuntu-archive-keyring.gpg")
        );
        assert_eq!(sources.entries[2].suite, "resolute-security");
    }

    #[test]
    fn skips_disabled_deb822_stanza() {
        let content = r#"Enabled: no
Types: deb
URIs: http://example.com/ubuntu
Suites: jammy
Components: main
"#;
        let sources = SourcesList::parse_deb822(content).unwrap();
        assert!(sources.entries.is_empty());
    }

    #[test]
    fn merges_supplemental_apt_sources() {
        let dir = std::env::temp_dir().join(format!("raptor-merge-sources-{}", std::process::id()));
        let list_d = dir.join("list.d");
        let sources_d = dir.join("sources.d");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&list_d).unwrap();
        fs::create_dir_all(&sources_d).unwrap();

        fs::write(
            sources_d.join("archive-ubuntu-com-ubuntu-resolute.yaml"),
            "kind: deb\nenabled: true\nuri: http://archive.ubuntu.com/ubuntu\nsuite: resolute\ncomponents:\n  - main\n",
        )
        .unwrap();
        fs::write(
            list_d.join("ubuntu.sources"),
            r#"Types: deb
URIs: http://archive.ubuntu.com/ubuntu
Suites: resolute
Components: main universe
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
"#,
        )
        .unwrap();
        fs::write(
            list_d.join("raptor-download-docker-com-linux-ubuntu.list"),
            "deb [signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu resolute stable\n",
        )
        .unwrap();

        std::env::set_var("RAPTOR_SOURCES_D", &sources_d);
        std::env::set_var("RAPTOR_SOURCES_LIST_D", &list_d);
        let sources = load_all_sources().unwrap();
        std::env::remove_var("RAPTOR_SOURCES_D");
        std::env::remove_var("RAPTOR_SOURCES_LIST_D");

        assert_eq!(sources.entries.len(), 3);
        assert!(sources
            .entries
            .iter()
            .any(|entry| entry.uri.contains("download.docker.com")));
        assert!(sources.entries.iter().any(|entry| {
            entry.uri.contains("archive.ubuntu.com")
                && entry.components.iter().any(|c| c == "universe")
        }));

        let _ = fs::remove_dir_all(&dir);
    }
}
