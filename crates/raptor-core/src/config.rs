use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Top-level Raptor runtime configuration (`/etc/raptor/config.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaptorConfig {
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub system: SystemConfig,
    #[serde(default)]
    pub unattended: UnattendedConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    #[serde(default = "default_state")]
    pub state: PathBuf,
    #[serde(default = "default_cache")]
    pub cache: PathBuf,
    #[serde(default = "default_archives")]
    pub archives: PathBuf,
    #[serde(default = "default_config_dir")]
    pub config_dir: PathBuf,
    #[serde(default = "default_sources")]
    pub sources: PathBuf,
    #[serde(default = "default_sources_list_d")]
    pub sources_list_d: PathBuf,
    #[serde(default = "default_keyrings")]
    pub keyrings: PathBuf,
    #[serde(default = "default_sources_d")]
    pub sources_d: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    #[serde(default)]
    pub suite: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub allow_insecure: bool,
    #[serde(default = "default_true")]
    pub debsig_verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnattendedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_hours")]
    pub interval_hours: u64,
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default = "default_true")]
    pub auto_upgrade: bool,
    #[serde(default)]
    pub auto_reboot: bool,
    /// Package name patterns to upgrade (empty = all upgradable packages).
    #[serde(default)]
    pub packages: Vec<String>,
    /// Limit upgrades to these origin URIs (empty = all signed sources).
    #[serde(default)]
    pub origins: Vec<String>,
}

impl Default for RaptorConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            system: SystemConfig::default(),
            unattended: UnattendedConfig::default(),
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            root: default_root(),
            state: default_state(),
            cache: default_cache(),
            archives: default_archives(),
            config_dir: default_config_dir(),
            sources: default_sources(),
            sources_list_d: default_sources_list_d(),
            keyrings: default_keyrings(),
            sources_d: default_sources_d(),
        }
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            suite: None,
            architecture: None,
            allow_insecure: false,
            debsig_verify: true,
        }
    }
}

impl Default for UnattendedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: default_interval_hours(),
            auto_update: true,
            auto_upgrade: true,
            auto_reboot: false,
            packages: Vec::new(),
            origins: Vec::new(),
        }
    }
}

pub fn default_config_path() -> PathBuf {
    PathBuf::from("/etc/raptor/config.yaml")
}

fn default_root() -> PathBuf {
    PathBuf::from("/")
}

fn default_state() -> PathBuf {
    PathBuf::from("/var/lib/dpkg/status")
}

fn default_cache() -> PathBuf {
    PathBuf::from("/var/cache/raptor")
}

fn default_archives() -> PathBuf {
    PathBuf::from("/var/cache/apt/archives")
}

fn default_config_dir() -> PathBuf {
    PathBuf::from("/etc/raptor")
}

fn default_sources() -> PathBuf {
    PathBuf::from("/etc/apt/sources.list")
}

fn default_sources_list_d() -> PathBuf {
    PathBuf::from("/etc/apt/sources.list.d")
}

fn default_keyrings() -> PathBuf {
    PathBuf::from("/etc/apt/keyrings")
}

fn default_sources_d() -> PathBuf {
    PathBuf::from("/etc/raptor/sources.d")
}

fn default_interval_hours() -> u64 {
    24
}

fn default_true() -> bool {
    true
}

pub fn load_yaml_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path).map_err(|e| {
        Error::Other(format!("reading {}: {e}", path.display()))
    })?;
    serde_yaml::from_str(&content).map_err(|e| {
        Error::Other(format!("invalid YAML in {}: {e}", path.display()))
    })
}

pub fn save_yaml_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(value).map_err(|e| {
        Error::Other(format!("serializing YAML for {}: {e}", path.display()))
    })?;
    fs::write(path, content)?;
    Ok(())
}

impl RaptorConfig {
    pub fn load() -> Result<Self> {
        Self::load_from(default_config_path())
    }

    pub fn load_from(path: PathBuf) -> Result<Self> {
        if path.exists() {
            load_yaml_file(&path)
        } else if let Ok(path) = std::env::var("RAPTOR_CONFIG") {
            load_yaml_file(Path::new(&path))
        } else {
            Ok(Self::from_env())
        }
    }

    /// Build config from legacy `RAPTOR_*` environment variables.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("RAPTOR_ROOT") {
            cfg.paths.root = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_STATE") {
            cfg.paths.state = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_CACHE") {
            cfg.paths.cache = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_ARCHIVES") {
            cfg.paths.archives = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_SOURCES") {
            cfg.paths.sources = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_SOURCES_LIST_D") {
            cfg.paths.sources_list_d = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_KEYRINGS_DIR") {
            cfg.paths.keyrings = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_SOURCES_D") {
            cfg.paths.sources_d = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_SUITE") {
            cfg.system.suite = Some(v);
        }
        if let Ok(v) = std::env::var("RAPTOR_ARCH") {
            cfg.system.architecture = Some(v);
        }
        if std::env::var("RAPTOR_ALLOW_INSECURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            cfg.system.allow_insecure = true;
        }
        if std::env::var("RAPTOR_DEBSIG_VERIFY")
            .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
        {
            cfg.system.debsig_verify = false;
        }
        cfg
    }

    pub fn apply_env(&self) {
        std::env::set_var("RAPTOR_ROOT", self.paths.root.to_string_lossy().as_ref());
        std::env::set_var("RAPTOR_STATE", self.paths.state.to_string_lossy().as_ref());
        std::env::set_var("RAPTOR_CACHE", self.paths.cache.to_string_lossy().as_ref());
        std::env::set_var("RAPTOR_ARCHIVES", self.paths.archives.to_string_lossy().as_ref());
        std::env::set_var("RAPTOR_SOURCES", self.paths.sources.to_string_lossy().as_ref());
        std::env::set_var(
            "RAPTOR_SOURCES_LIST_D",
            self.paths.sources_list_d.to_string_lossy().as_ref(),
        );
        std::env::set_var("RAPTOR_KEYRINGS_DIR", self.paths.keyrings.to_string_lossy().as_ref());
        std::env::set_var(
            "RAPTOR_SOURCES_D",
            self.paths.sources_d.to_string_lossy().as_ref(),
        );
        if let Some(suite) = &self.system.suite {
            std::env::set_var("RAPTOR_SUITE", suite);
        }
        if let Some(arch) = &self.system.architecture {
            std::env::set_var("RAPTOR_ARCH", arch);
        }
        if self.system.allow_insecure {
            std::env::set_var("RAPTOR_ALLOW_INSECURE", "1");
        }
        if !self.system.debsig_verify {
            std::env::set_var("RAPTOR_DEBSIG_VERIFY", "0");
        }
    }

    /// Template for `raptor config init` with unattended upgrades enabled.
    pub fn write_init_template(path: &Path) -> Result<()> {
        let mut cfg = Self::default();
        cfg.unattended.enabled = true;
        save_yaml_file(path, &cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_yaml() {
        let yaml = r#"
paths:
  root: /
  state: /var/lib/dpkg/status
system:
  suite: jammy
  allow_insecure: false
unattended:
  enabled: true
  interval_hours: 12
  auto_upgrade: true
"#;
        let cfg: RaptorConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.system.suite.as_deref(), Some("jammy"));
        assert!(cfg.unattended.enabled);
        assert_eq!(cfg.unattended.interval_hours, 12);
    }
}
