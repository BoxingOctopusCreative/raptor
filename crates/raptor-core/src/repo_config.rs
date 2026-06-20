use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{load_yaml_file, save_yaml_file};
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoKind {
    Private,
    Ppa,
    Mirror,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub kind: RepoKind,
    pub suite: String,
    #[serde(default)]
    pub codename: Option<String>,
    #[serde(default = "default_components")]
    pub components: Vec<String>,
    #[serde(default = "default_architectures")]
    pub architectures: Vec<String>,
    #[serde(default)]
    pub signing: Option<SigningConfig>,
    #[serde(default)]
    pub ppa: Option<PpaRepoConfig>,
    #[serde(default)]
    pub mirror: Option<MirrorSection>,
    #[serde(default)]
    pub publish: PublishConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningConfig {
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default = "default_keyring_path")]
    pub keyring: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PpaRepoConfig {
    pub owner: String,
    pub name: String,
    #[serde(default)]
    pub launchpad_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorSection {
    pub upstream: String,
    #[serde(default)]
    pub sync_pool: bool,
    #[serde(default)]
    pub architectures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishConfig {
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default = "default_label")]
    pub label: String,
}

fn default_components() -> Vec<String> {
    vec!["main".into()]
}

fn default_architectures() -> Vec<String> {
    vec!["amd64".into(), "arm64".into(), "all".into()]
}

fn default_keyring_path() -> String {
    "keyrings/repo.gpg".into()
}

fn default_origin() -> String {
    "Raptor".into()
}

fn default_label() -> String {
    "Raptor Repository".into()
}

impl RepoConfig {
    pub const FILE_NAME: &'static str = "repo.yaml";

    pub fn load(path: &Path) -> Result<Self> {
        load_yaml_file(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_yaml_file(path, self)
    }

    pub fn codename(&self) -> &str {
        self.codename.as_deref().unwrap_or(&self.suite)
    }

    pub fn private(suite: &str, component: &str) -> Self {
        Self {
            kind: RepoKind::Private,
            suite: suite.into(),
            codename: Some(suite.into()),
            components: vec![component.into()],
            architectures: default_architectures(),
            signing: Some(SigningConfig {
                key_id: None,
                keyring: default_keyring_path(),
                email: Some("repo@example.com".into()),
            }),
            ppa: None,
            mirror: None,
            publish: PublishConfig::default(),
        }
    }

    pub fn ppa(owner: &str, name: &str, suite: &str) -> Self {
        Self {
            kind: RepoKind::Ppa,
            suite: suite.into(),
            codename: Some(suite.into()),
            components: vec!["main".into()],
            architectures: default_architectures(),
            signing: Some(SigningConfig {
                key_id: None,
                keyring: format!("keyrings/{owner}-ubuntu-{name}.gpg"),
                email: None,
            }),
            ppa: Some(PpaRepoConfig {
                owner: owner.into(),
                name: name.into(),
                launchpad_uri: Some(format!(
                    "https://ppa.launchpadcontent.net/{owner}/{name}/ubuntu"
                )),
            }),
            mirror: None,
            publish: PublishConfig {
                origin: format!("LP-PPA-{owner}-{name}"),
                label: format!("{owner}/{name} PPA"),
            },
        }
    }
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            origin: default_origin(),
            label: default_label(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_repo_defaults() {
        let cfg = RepoConfig::private("stable", "main");
        assert_eq!(cfg.kind, RepoKind::Private);
        assert_eq!(cfg.components, vec!["main"]);
    }
}
