use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{load_yaml_file, save_yaml_file};
use crate::error::Result;

/// Package build manifest (`raptor.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: ManifestPackage,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub data: Option<ManifestData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPackage {
    pub name: String,
    pub version: String,
    #[serde(default = "default_arch")]
    pub architecture: String,
    #[serde(default)]
    pub maintainer: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends: String,
    #[serde(default)]
    pub section: String,
    #[serde(default = "default_priority")]
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestData {
    #[serde(default = "default_data_dir")]
    pub source: String,
    #[serde(default)]
    pub dest_prefix: String,
}

fn default_arch() -> String {
    "all".into()
}

fn default_priority() -> String {
    "optional".into()
}

fn default_data_dir() -> String {
    "data".into()
}

impl PackageManifest {
    pub fn load(path: &Path) -> Result<Self> {
        load_yaml_file(path)
    }

    pub fn write_default(path: &Path, name: &str, version: &str, arch: &str) -> Result<()> {
        let manifest = Self {
            package: ManifestPackage {
                name: name.into(),
                version: version.into(),
                architecture: arch.into(),
                maintainer: "Maintainer <maintainer@example.com>".into(),
                description: "Short package description".into(),
                depends: String::new(),
                section: "utils".into(),
                priority: "optional".into(),
            },
            files: Vec::new(),
            data: Some(ManifestData {
                source: "data".into(),
                dest_prefix: "usr/local".into(),
            }),
        };
        save_yaml_file(path, &manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_yaml() {
        let yaml = r#"
package:
  name: hello
  version: 1.0.0
  architecture: all
data:
  source: data
  dest_prefix: usr/local
"#;
        let m: PackageManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.package.name, "hello");
        assert_eq!(m.data.unwrap().source, "data");
    }
}
