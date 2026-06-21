use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::control::ControlFile;
use crate::dpkg_status::{is_installed_status, is_tracked_status, load_status, write_status};
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

#[derive(Debug, Default)]
pub struct State {
    pub packages: HashMap<String, InstalledPackage>,
    pub file_path: Option<PathBuf>,
    dpkg_stanzas: BTreeMap<String, BTreeMap<String, String>>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                packages: HashMap::new(),
                file_path: Some(path.to_path_buf()),
                dpkg_stanzas: BTreeMap::new(),
            });
        }

        if is_dpkg_status_path(path) {
            return Self::load_dpkg_status(path);
        }

        let content = fs::read_to_string(path)?;
        let mut state: State = if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
            serde_yaml::from_str(&content).unwrap_or_default()
        } else {
            serde_json::from_str(&content).unwrap_or_default()
        };
        state.file_path = Some(path.to_path_buf());
        Ok(state)
    }

    fn load_dpkg_status(path: &Path) -> Result<Self> {
        let stanzas = load_status(path)?;
        let mut packages = HashMap::new();

        for (name, fields) in &stanzas {
            let status = fields.get("Status").map(String::as_str).unwrap_or_default();
            if !is_tracked_status(status) {
                continue;
            }
            packages.insert(
                name.clone(),
                InstalledPackage {
                    name: name.clone(),
                    version: fields.get("Version").cloned().unwrap_or_default(),
                    architecture: fields.get("Architecture").cloned().unwrap_or_default(),
                    status: status.to_string(),
                    maintainer: fields.get("Maintainer").cloned().unwrap_or_default(),
                    description: fields.get("Description").cloned().unwrap_or_default(),
                },
            );
        }

        Ok(Self {
            packages,
            file_path: Some(path.to_path_buf()),
            dpkg_stanzas: stanzas,
        })
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.file_path else {
            return Ok(());
        };

        if is_dpkg_status_path(path) {
            return self.save_dpkg_status(path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
            serde_yaml::to_string(self)?
        } else {
            serde_json::to_string_pretty(self)?
        };
        fs::write(path, content)?;
        Ok(())
    }

    fn save_dpkg_status(&self, path: &Path) -> Result<()> {
        let mut stanzas = self.dpkg_stanzas.clone();

        for pkg in self.packages.values() {
            let fields = stanzas.entry(pkg.name.clone()).or_default();
            fields.insert("Package".into(), pkg.name.clone());
            fields.insert("Status".into(), pkg.status.clone());
            fields.insert("Version".into(), pkg.version.clone());
            fields.insert("Architecture".into(), pkg.architecture.clone());
            if !pkg.maintainer.is_empty() {
                fields.insert("Maintainer".into(), pkg.maintainer.clone());
            }
            if !pkg.description.is_empty() {
                fields.insert("Description".into(), pkg.description.clone());
            }
        }

        write_status(path, &stanzas)
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.packages
            .get(name)
            .is_some_and(|p| is_installed_status(&p.status))
    }

    pub fn is_removable(&self, name: &str) -> bool {
        self.is_installed(name)
    }

    pub fn is_purgeable(&self, name: &str) -> bool {
        self.packages.get(name).is_some_and(|p| {
            is_installed_status(&p.status) || p.status.starts_with("deinstall ok config-files")
        })
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
        if let Some(pkg) = self.packages.get_mut(name) {
            if is_installed_status(&pkg.status) {
                pkg.status = "deinstall ok config-files".into();
                return true;
            }
        }
        false
    }

    pub fn purge(&mut self, name: &str) -> bool {
        if self.packages.remove(name).is_some() {
            self.dpkg_stanzas.remove(name);
            return true;
        }
        false
    }

    pub fn get(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.get(name)
    }

    pub fn installed_names(&self) -> Vec<String> {
        self.packages
            .values()
            .filter(|p| is_installed_status(&p.status))
            .map(|p| p.name.clone())
            .collect()
    }
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.packages.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let packages = HashMap::<String, InstalledPackage>::deserialize(deserializer)?;
        Ok(Self {
            packages,
            file_path: None,
            dpkg_stanzas: BTreeMap::new(),
        })
    }
}

fn is_dpkg_status_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "status" || name == "status.d")
}

pub fn default_state_path() -> PathBuf {
    PathBuf::from("/var/lib/dpkg/status")
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
    if let Ok(v) = std::env::var("RAPTOR_ARCH") {
        return v;
    }
    if let Some(arch) = read_dpkg_native_arch() {
        return arch;
    }
    std::env::consts::ARCH.to_string()
}

fn read_dpkg_native_arch() -> Option<String> {
    let arch = std::fs::read_to_string("/var/lib/dpkg/arch")
        .ok()?
        .trim()
        .to_string();
    if arch.is_empty() {
        None
    } else {
        Some(arch)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_saves_dpkg_status() {
        let dir = std::env::temp_dir().join(format!("raptor-dpkg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status");

        fs::write(
            &path,
            "Package: hello\nStatus: install ok installed\nVersion: 1.0\nArchitecture: all\nMaintainer: Demo\nDescription: hi\n\n",
        )
        .unwrap();

        let state = State::load(&path).unwrap();
        assert!(state.is_installed("hello"));

        let mut updated = state;
        updated.install(&ControlFile {
            package: "world".into(),
            version: "2.0".into(),
            architecture: "all".into(),
            maintainer: "Demo".into(),
            description: "world pkg".into(),
            ..Default::default()
        });
        updated.save().unwrap();

        let reloaded = State::load(&path).unwrap();
        assert!(reloaded.is_installed("hello"));
        assert!(reloaded.is_installed("world"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reloads_config_files_status_for_purge() {
        let dir = std::env::temp_dir().join(format!("raptor-dpkg-purge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status");

        fs::write(
            &path,
            "Package: hello\nStatus: deinstall ok config-files\nVersion: 1.0\nArchitecture: all\nMaintainer: Demo\nDescription: hi\n\n",
        )
        .unwrap();

        let state = State::load(&path).unwrap();
        assert!(!state.is_installed("hello"));
        assert!(state.is_purgeable("hello"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_preserves_conffiles_format_for_other_packages() {
        let dir = std::env::temp_dir().join(format!("raptor-dpkg-conffiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status");

        fs::write(
            &path,
            "\
Package: adduser
Status: install ok installed
Version: 3.137ubuntu1
Architecture: all
Conffiles:
 /etc/adduser.conf deadbeef0123456789deadbeef0123456789
Description: add and remove users

Package: hello
Status: install ok installed
Version: 1.0
Architecture: all
Description: hi

",
        )
        .unwrap();

        let mut state = State::load(&path).unwrap();
        state.install(&ControlFile {
            package: "raptor".into(),
            version: "0.6.3".into(),
            architecture: "arm64".into(),
            ..Default::default()
        });
        state.save().unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("Conffiles:\n /etc/adduser.conf"));
        assert!(!saved.contains("Conffiles: /etc/"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deb_architecture_maps_rust_targets() {
        assert_eq!(deb_architecture("x86_64"), "amd64");
        assert_eq!(deb_architecture("aarch64"), "arm64");
        assert_eq!(deb_architecture("amd64"), "amd64");
    }
}
