use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::control::ControlFile;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub status: String,
    pub maintainer: String,
    pub description: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub packages: HashMap<String, InstalledPackage>,
    #[serde(skip)]
    pub file_path: Option<PathBuf>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let mut state: State = if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                serde_yaml::from_str(&content).unwrap_or_default()
            } else {
                serde_json::from_str(&content).unwrap_or_default()
            };
            state.file_path = Some(path.to_path_buf());
            Ok(state)
        } else {
            Ok(Self {
                packages: HashMap::new(),
                file_path: Some(path.to_path_buf()),
            })
        }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(path) = &self.file_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                serde_yaml::to_string(self)?
            } else {
                serde_json::to_string_pretty(self)?
            };
            fs::write(path, content)?;
        }
        Ok(())
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.packages
            .get(name)
            .is_some_and(|p| p.status.starts_with("install"))
    }

    pub fn install(&mut self, control: &ControlFile) {
        self.packages.insert(
            control.package.clone(),
            InstalledPackage {
                name: control.package.clone(),
                version: control.version.clone(),
                architecture: control.architecture.clone(),
                status: "install ok installed".into(),
                maintainer: control.maintainer.clone(),
                description: control.description.clone(),
            },
        );
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.packages.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.get(name)
    }

    pub fn installed_names(&self) -> Vec<String> {
        self.packages
            .values()
            .filter(|p| p.status.starts_with("install"))
            .map(|p| p.name.clone())
            .collect()
    }
}

pub fn default_state_path() -> PathBuf {
    PathBuf::from("/var/lib/raptor/state.yaml")
}

pub fn default_install_root() -> PathBuf {
    PathBuf::from("/")
}

pub fn default_cache_dir() -> PathBuf {
    PathBuf::from("/var/cache/raptor")
}

pub fn default_archives_dir() -> PathBuf {
    std::env::var("RAPTOR_ARCHIVES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/cache/apt/archives"))
}

pub fn detect_architecture() -> String {
    std::env::var("RAPTOR_ARCH").unwrap_or_else(|_| std::env::consts::ARCH.to_string())
}

/// Map Rust architecture names to Debian binary architecture names.
pub fn deb_architecture(arch: &str) -> String {
    match arch {
        "x86_64" => "amd64".into(),
        "aarch64" => "arm64".into(),
        "arm" => "armhf".into(),
        other => other.to_string(),
    }
}
