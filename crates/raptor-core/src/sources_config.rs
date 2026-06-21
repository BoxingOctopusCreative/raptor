use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{load_yaml_file, save_yaml_file, RaptorConfig};
use crate::error::Result;
use crate::ppa::PpaRef;
use crate::sources::{is_apt_sources_file, SourceEntry, SourceType, SourcesList};

pub const DEFAULT_SOURCES_D: &str = "/etc/raptor/sources.d";

pub const DEFAULT_REPO_PRIORITY: i32 = 500;

/// One or more repositories in a single YAML file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourcesYaml {
    #[serde(default)]
    pub repositories: Vec<RepositoryEntry>,
}

/// A single repository definition. Used directly in per-repo files under `sources.d`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryEntry {
    pub kind: RepositoryKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub uri: String,
    pub suite: String,
    pub components: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppa: Option<PpaReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Pin priority for this repository (higher wins when versions tie).
    #[serde(default = "default_repo_priority")]
    pub priority: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryKind {
    Deb,
    DebSrc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PpaReference {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SourceFileDocument {
    Single(RepositoryEntry),
    Multi(SourcesYaml),
}

fn default_true() -> bool {
    true
}

fn default_repo_priority() -> i32 {
    DEFAULT_REPO_PRIORITY
}

impl SourcesYaml {
    pub fn load(path: &Path) -> Result<Self> {
        load_yaml_file(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_yaml_file(path, self)
    }

    pub fn from_sources_list(list: &SourcesList) -> Self {
        Self {
            repositories: list
                .entries
                .iter()
                .map(RepositoryEntry::from_source_entry)
                .collect(),
        }
    }

    pub fn to_sources_list(&self) -> SourcesList {
        SourcesList {
            entries: self
                .repositories
                .iter()
                .map(RepositoryEntry::to_source_entry)
                .collect(),
        }
    }
}

impl RepositoryEntry {
    pub fn save(&self, path: &Path) -> Result<()> {
        save_yaml_file(path, self)
    }

    pub fn from_source_entry(entry: &SourceEntry) -> Self {
        Self {
            kind: match entry.source_type {
                SourceType::Deb => RepositoryKind::Deb,
                SourceType::DebSrc => RepositoryKind::DebSrc,
            },
            enabled: entry.enabled,
            uri: entry.uri.clone(),
            suite: entry.suite.clone(),
            components: entry.components.clone(),
            signed_by: entry.signed_by.clone(),
            ppa: ppa_reference_from_uri(&entry.uri),
            origin: None,
            priority: entry.priority,
        }
    }

    pub fn from_source_entry_with_origin(entry: &SourceEntry, origin: PathBuf) -> Self {
        let mut repo = Self::from_source_entry(entry);
        repo.origin = Some(origin.display().to_string());
        repo
    }

    fn to_source_entry(&self) -> SourceEntry {
        SourceEntry {
            source_type: match self.kind {
                RepositoryKind::Deb => SourceType::Deb,
                RepositoryKind::DebSrc => SourceType::DebSrc,
            },
            uri: self.uri.clone(),
            suite: self.suite.clone(),
            components: self.components.clone(),
            architectures: Vec::new(),
            enabled: self.enabled,
            signed_by: self.signed_by.clone(),
            priority: self.priority,
        }
    }
}

pub fn default_sources_d_path() -> PathBuf {
    PathBuf::from(DEFAULT_SOURCES_D)
}

pub fn load_sources_from_dir(dir: &Path) -> Result<SourcesList> {
    let mut yaml = SourcesYaml::default();

    if !dir.is_dir() {
        return Ok(yaml.to_sources_list());
    }

    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    files.sort();

    for path in files {
        yaml.repositories.extend(load_source_file(&path)?.repositories);
    }

    Ok(yaml.to_sources_list())
}

pub fn load_source_file(path: &Path) -> Result<SourcesYaml> {
    let doc: SourceFileDocument = load_yaml_file(path)?;
    Ok(match doc {
        SourceFileDocument::Single(entry) => SourcesYaml {
            repositories: vec![entry],
        },
        SourceFileDocument::Multi(multi) => multi,
    })
}

pub fn write_sources_to_dir(dir: &Path, repositories: &[RepositoryEntry]) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    for repo in repositories {
        let filename = source_file_name(repo);
        let path = dir.join(filename);
        repo.save(&path)?;
        written.push(path);
    }

    Ok(written)
}

pub fn collect_apt_source_files(main: &Path, list_d: &Path) -> Result<Vec<(PathBuf, SourcesList)>> {
    let mut files = Vec::new();

    if main.exists() {
        files.push((main.to_path_buf(), SourcesList::load(main)?));
    }

    if list_d.is_dir() {
        let mut list_paths: Vec<PathBuf> = fs::read_dir(list_d)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| is_apt_sources_file(path))
            .collect();
        list_paths.sort();
        for path in list_paths {
            files.push((path.clone(), SourcesList::load(&path)?));
        }
    }

    Ok(files)
}

pub fn convert_apt_sources(main: &Path, list_d: &Path) -> Result<Vec<RepositoryEntry>> {
    let mut repositories = Vec::new();
    let mut index = 0;

    for (path, list) in collect_apt_source_files(main, list_d)? {
        for entry in list.entries {
            let mut repo = RepositoryEntry::from_source_entry_with_origin(&entry, path.clone());
            repo.priority = DEFAULT_REPO_PRIORITY - index * 10;
            index += 1;
            repositories.push(repo);
        }
    }

    Ok(repositories)
}

/// Stable identifier for a repository (matches the basename of its YAML file without extension).
pub fn repository_id(repo: &RepositoryEntry) -> String {
    source_file_name(repo)
        .trim_end_matches(".yaml")
        .to_string()
}

#[derive(Debug, Clone)]
pub struct ConfiguredRepository {
    pub id: String,
    pub path: PathBuf,
    pub entry: RepositoryEntry,
}

enum LoadedSourceFile {
    Single(RepositoryEntry),
    Multi(SourcesYaml),
}

impl LoadedSourceFile {
    fn repositories(&self) -> Vec<&RepositoryEntry> {
        match self {
            LoadedSourceFile::Single(entry) => vec![entry],
            LoadedSourceFile::Multi(yaml) => yaml.repositories.iter().collect(),
        }
    }

    fn repositories_mut(&mut self) -> Vec<&mut RepositoryEntry> {
        match self {
            LoadedSourceFile::Single(entry) => vec![entry],
            LoadedSourceFile::Multi(yaml) => yaml.repositories.iter_mut().collect(),
        }
    }
}

fn load_source_file_any(path: &Path) -> Result<LoadedSourceFile> {
    let doc: SourceFileDocument = load_yaml_file(path)?;
    Ok(match doc {
        SourceFileDocument::Single(entry) => LoadedSourceFile::Single(entry),
        SourceFileDocument::Multi(multi) => LoadedSourceFile::Multi(multi),
    })
}

fn save_source_file_any(path: &Path, file: &LoadedSourceFile) -> Result<()> {
    match file {
        LoadedSourceFile::Single(entry) => entry.save(path),
        LoadedSourceFile::Multi(yaml) => yaml.save(path),
    }
}

fn list_source_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    files.sort();
    Ok(files)
}

pub fn list_configured_repositories(dir: &Path) -> Result<Vec<ConfiguredRepository>> {
    let mut configured = Vec::new();

    for path in list_source_files(dir)? {
        let file = load_source_file_any(&path)?;
        for entry in file.repositories() {
            configured.push(ConfiguredRepository {
                id: repository_id(entry),
                path: path.clone(),
                entry: entry.clone(),
            });
        }
    }

    configured.sort_by(|a, b| {
        b.entry
            .priority
            .cmp(&a.entry.priority)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(configured)
}

pub fn set_repository_priority(dir: &Path, id: &str, priority: i32) -> Result<()> {
    for path in list_source_files(dir)? {
        let mut file = load_source_file_any(&path)?;
        let mut updated = false;
        for entry in file.repositories_mut() {
            if repository_id(entry) == id {
                entry.priority = priority;
                updated = true;
            }
        }
        if updated {
            save_source_file_any(&path, &file)?;
            return Ok(());
        }
    }

    Err(crate::error::Error::InvalidSources(format!(
        "repository not found: {id}"
    )))
}

pub fn reorder_repositories(dir: &Path, ordered_ids: &[String]) -> Result<()> {
    if ordered_ids.is_empty() {
        return Err(crate::error::Error::InvalidSources(
            "no repository ids provided".into(),
        ));
    }

    let configured = list_configured_repositories(dir)?;
    let known: std::collections::HashSet<_> = configured.iter().map(|repo| repo.id.as_str()).collect();
    for id in ordered_ids {
        if !known.contains(id.as_str()) {
            return Err(crate::error::Error::InvalidSources(format!(
                "repository not found: {id}"
            )));
        }
    }

    let base = 1000_i32;
    let mut priorities = std::collections::HashMap::new();
    for (index, id) in ordered_ids.iter().enumerate() {
        priorities.insert(id.clone(), base - index as i32 * 10);
    }

    let mut touched_paths = std::collections::HashSet::new();
    for path in list_source_files(dir)? {
        let mut file = load_source_file_any(&path)?;
        let mut updated = false;
        for entry in file.repositories_mut() {
            if let Some(priority) = priorities.get(&repository_id(entry)) {
                entry.priority = *priority;
                updated = true;
            }
        }
        if updated {
            save_source_file_any(&path, &file)?;
            touched_paths.insert(path);
        }
    }

    if touched_paths.is_empty() {
        return Err(crate::error::Error::InvalidSources(
            "no repositories updated".into(),
        ));
    }

    Ok(())
}

pub fn convert_apt_sources_from_config() -> Result<Vec<RepositoryEntry>> {
    let config = RaptorConfig::load().unwrap_or_default();
    convert_apt_sources(&config.paths.sources, &config.paths.sources_list_d)
}

fn source_file_name(repo: &RepositoryEntry) -> String {
    if let Some(ppa) = &repo.ppa {
        return format!("ppa-{}-{}.yaml", ppa.owner, ppa.name);
    }

    let slug = repo
        .uri
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("file:")
        .replace(['/', ':', '.'], "-")
        .trim_matches('-')
        .to_string();
    let suite = repo
        .suite
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("{slug}-{suite}.yaml")
}

fn ppa_reference_from_uri(uri: &str) -> Option<PpaReference> {
    parse_ppa_uri(uri).ok().map(|ppa| PpaReference {
        owner: ppa.owner,
        name: ppa.name,
    })
}

fn parse_ppa_uri(uri: &str) -> Result<PpaRef> {
    let rest = uri
        .split("ppa.launchpadcontent.net/")
        .nth(1)
        .or_else(|| uri.split("ppa.launchpad.net/").nth(1))
        .ok_or_else(|| crate::error::Error::InvalidSources(format!("not a PPA URI: {uri}")))?;
    let mut parts = rest.split('/');
    let owner = parts
        .next()
        .ok_or_else(|| crate::error::Error::InvalidSources(format!("invalid PPA URI: {uri}")))?;
    let name = parts
        .next()
        .ok_or_else(|| crate::error::Error::InvalidSources(format!("invalid PPA URI: {uri}")))?;
    Ok(PpaRef {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourcesList;

    #[test]
    fn converts_signed_by_source_line() {
        let content = "deb [signed-by=/etc/apt/keyrings/example.gpg] https://example.com/ubuntu jammy main universe\n";
        let list = SourcesList::parse(content).unwrap();
        let yaml = SourcesYaml::from_sources_list(&list);

        assert_eq!(yaml.repositories.len(), 1);
        assert_eq!(yaml.repositories[0].uri, "https://example.com/ubuntu");
        assert_eq!(yaml.repositories[0].suite, "jammy");
        assert_eq!(yaml.repositories[0].components, vec!["main", "universe"]);
    }

    #[test]
    fn loads_per_repo_file_format() {
        let dir = std::env::temp_dir().join(format!("raptor-sources-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file = dir.join("local.yaml");
        fs::write(
            &file,
            "kind: deb\nenabled: true\nuri: file:/var/local/repo\nsuite: stable\ncomponents:\n  - main\n",
        )
        .unwrap();

        let list = load_sources_from_dir(&dir).unwrap();
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].uri, "file:/var/local/repo");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_one_file_per_repository() {
        let dir = std::env::temp_dir().join(format!("raptor-sources-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let repos = vec![
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "http://archive.ubuntu.com/ubuntu".into(),
                suite: "jammy".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: None,
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "https://ppa.launchpadcontent.net/git-core/cargo/ubuntu".into(),
                suite: "jammy".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: Some(PpaReference {
                    owner: "git-core".into(),
                    name: "cargo".into(),
                }),
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
        ];

        let written = write_sources_to_dir(&dir, &repos).unwrap();
        assert_eq!(written.len(), 2);
        assert!(written.iter().any(|p| {
            p.file_name().is_some_and(|n| n == "archive-ubuntu-com-ubuntu-jammy.yaml")
        }));
        assert!(written
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "ppa-git-core-cargo.yaml")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_distinct_files_per_suite_for_same_uri() {
        let dir = std::env::temp_dir().join(format!(
            "raptor-sources-write-suites-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);

        let repos = vec![
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "http://archive.ubuntu.com/ubuntu".into(),
                suite: "resolute".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: None,
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "http://archive.ubuntu.com/ubuntu".into(),
                suite: "resolute-updates".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: None,
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
        ];

        let written = write_sources_to_dir(&dir, &repos).unwrap();
        assert_eq!(written.len(), 2);
        assert!(written.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n == "archive-ubuntu-com-ubuntu-resolute.yaml")
        }));
        assert!(written.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n == "archive-ubuntu-com-ubuntu-resolute-updates.yaml")
        }));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sets_and_reorders_repository_priority() {
        let dir = std::env::temp_dir().join(format!("raptor-priority-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let repos = vec![
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "http://security.ubuntu.com/ubuntu".into(),
                suite: "jammy-security".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: None,
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
            RepositoryEntry {
                kind: RepositoryKind::Deb,
                enabled: true,
                uri: "http://archive.ubuntu.com/ubuntu".into(),
                suite: "jammy".into(),
                components: vec!["main".into()],
                signed_by: None,
                ppa: None,
                origin: None,
                priority: DEFAULT_REPO_PRIORITY,
            },
        ];
        write_sources_to_dir(&dir, &repos).unwrap();

        set_repository_priority(
            &dir,
            "archive-ubuntu-com-ubuntu-jammy",
            900,
        )
        .unwrap();

        let listed = list_configured_repositories(&dir).unwrap();
        assert_eq!(listed[0].id, "archive-ubuntu-com-ubuntu-jammy");
        assert_eq!(listed[0].entry.priority, 900);

        reorder_repositories(
            &dir,
            &[
                "security-ubuntu-com-ubuntu-jammy-security".into(),
                "archive-ubuntu-com-ubuntu-jammy".into(),
            ],
        )
        .unwrap();

        let listed = list_configured_repositories(&dir).unwrap();
        assert_eq!(listed[0].id, "security-ubuntu-com-ubuntu-jammy-security");
        assert_eq!(listed[0].entry.priority, 1000);
        assert_eq!(listed[1].entry.priority, 990);

        let _ = fs::remove_dir_all(&dir);
    }
}
