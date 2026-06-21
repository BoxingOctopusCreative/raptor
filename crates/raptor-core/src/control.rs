use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dependency::{parse_dependency_groups, parse_dependency_list, Dependency};
use crate::error::{Error, Result};

/// Debian control file fields for a binary package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFile {
    pub package: String,
    pub version: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub maintainer: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "Description")]
    pub description_long: Option<String>,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub depends: String,
    #[serde(default)]
    pub predepends: String,
    #[serde(default)]
    pub recommends: String,
    #[serde(default)]
    pub suggests: String,
    #[serde(default)]
    pub conflicts: String,
    #[serde(default)]
    pub breaks: String,
    #[serde(default)]
    pub replaces: String,
    #[serde(default)]
    pub provides: String,
    #[serde(default)]
    pub installed_size: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub md5sum: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, String>,
}

impl ControlFile {
    pub fn parse(content: &str) -> Result<Self> {
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        let mut current_key: Option<String> = None;

        for line in content.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                let key = current_key.as_ref().ok_or_else(|| {
                    Error::InvalidControl("continuation without key".into())
                })?;
                let entry = fields.get_mut(key).unwrap();
                if !entry.is_empty() {
                    entry.push('\n');
                }
                entry.push_str(line.trim_start());
                continue;
            }

            let Some((key, value)) = line.split_once(':') else {
                if line.trim().is_empty() {
                    continue;
                }
                return Err(Error::InvalidControl(format!("invalid line: {line}")));
            };

            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            current_key = Some(key.clone());
            fields.insert(key, value);
        }

        let mut control = ControlFile::default();
        for (key, value) in fields {
            match key.as_str() {
                "package" => control.package = value,
                "version" => control.version = value,
                "architecture" => control.architecture = value,
                "maintainer" => control.maintainer = value,
                "description" => control.description = value,
                "section" => control.section = value,
                "priority" => control.priority = value,
                "homepage" => control.homepage = value,
                "depends" => control.depends = value,
                "predepends" => control.predepends = value,
                "recommends" => control.recommends = value,
                "suggests" => control.suggests = value,
                "conflicts" => control.conflicts = value,
                "breaks" => control.breaks = value,
                "replaces" => control.replaces = value,
                "provides" => control.provides = value,
                "installed-size" => control.installed_size = value,
                "filename" => control.filename = value,
                "size" => control.size = value,
                "md5sum" => control.md5sum = value,
                "sha256" => control.sha256 = value,
                _ => {
                    control.extra.insert(key, value);
                }
            }
        }

        if control.package.is_empty() {
            return Err(Error::InvalidControl("missing Package field".into()));
        }
        if control.version.is_empty() {
            return Err(Error::InvalidControl("missing Version field".into()));
        }

        Ok(control)
    }

    pub fn to_string(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Package: {}", self.package));
        lines.push(format!("Version: {}", self.version));
        if !self.architecture.is_empty() {
            lines.push(format!("Architecture: {}", self.architecture));
        }
        if !self.maintainer.is_empty() {
            lines.push(format!("Maintainer: {}", self.maintainer));
        }
        if !self.description.is_empty() {
            lines.push(format!("Description: {}", self.description));
        }
        if !self.section.is_empty() {
            lines.push(format!("Section: {}", self.section));
        }
        if !self.priority.is_empty() {
            lines.push(format!("Priority: {}", self.priority));
        }
        if !self.homepage.is_empty() {
            lines.push(format!("Homepage: {}", self.homepage));
        }
        if !self.depends.is_empty() {
            lines.push(format!("Depends: {}", self.depends));
        }
        if !self.predepends.is_empty() {
            lines.push(format!("Pre-Depends: {}", self.predepends));
        }
        if !self.recommends.is_empty() {
            lines.push(format!("Recommends: {}", self.recommends));
        }
        if !self.suggests.is_empty() {
            lines.push(format!("Suggests: {}", self.suggests));
        }
        if !self.conflicts.is_empty() {
            lines.push(format!("Conflicts: {}", self.conflicts));
        }
        if !self.breaks.is_empty() {
            lines.push(format!("Breaks: {}", self.breaks));
        }
        if !self.replaces.is_empty() {
            lines.push(format!("Replaces: {}", self.replaces));
        }
        if !self.provides.is_empty() {
            lines.push(format!("Provides: {}", self.provides));
        }
        if !self.installed_size.is_empty() {
            lines.push(format!("Installed-Size: {}", self.installed_size));
        }
        for (key, value) in &self.extra {
            let title = title_case_field(key);
            lines.push(format!("{title}: {value}"));
        }
        lines.join("\n") + "\n"
    }

    pub fn depends_list(&self) -> Vec<Dependency> {
        parse_dependency_list(&self.depends)
    }

    pub fn depends_groups(&self) -> Vec<Vec<Dependency>> {
        parse_dependency_groups(&self.depends)
    }

    pub fn predepends_list(&self) -> Vec<Dependency> {
        parse_dependency_list(&self.predepends)
    }

    pub fn predepends_groups(&self) -> Vec<Vec<Dependency>> {
        parse_dependency_groups(&self.predepends)
    }

    pub fn conflicts_list(&self) -> Vec<Dependency> {
        parse_dependency_list(&self.conflicts)
    }

    pub fn provides_list(&self) -> Vec<String> {
        self.provides
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(parse_provides_name)
            .collect()
    }

    pub fn full_name(&self) -> String {
        format!("{}_{}_{}.deb", self.package, self.version, self.architecture)
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }
}

fn parse_provides_name(input: &str) -> String {
    input.split('(').next().unwrap_or(input).trim().to_string()
}

fn title_case_field(key: &str) -> String {
    key.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut s = first.to_uppercase().to_string();
                    s.push_str(chars.as_str());
                    s
                }
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_description() {
        let content = "Package: hello\nVersion: 1.0\nDescription: short\n long description\n";
        let control = ControlFile::parse(content).unwrap();
        assert_eq!(control.package, "hello");
        assert_eq!(control.description, "short\nlong description");
    }
}
