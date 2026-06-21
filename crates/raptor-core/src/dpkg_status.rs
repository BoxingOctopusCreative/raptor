use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

/// Parse `/var/lib/dpkg/status` into stanzas keyed by package name.
pub fn parse_status_file(content: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut stanzas = BTreeMap::new();

    for stanza in content.split("\n\n") {
        let stanza = stanza.trim();
        if stanza.is_empty() {
            continue;
        }
        let fields = parse_stanza(stanza)?;
        if let Some(name) = fields.get("Package") {
            stanzas.insert(name.clone(), fields);
        }
    }

    Ok(stanzas)
}

pub fn load_status(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let content = fs::read_to_string(path)?;
    parse_status_file(&content)
}

pub fn write_status(path: &Path, stanzas: &BTreeMap<String, BTreeMap<String, String>>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut output = String::new();
    for (i, fields) in stanzas.values().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        output.push_str(&format_stanza(fields));
        output.push('\n');
    }

    fs::write(path, output)?;
    Ok(())
}

pub fn is_installed_status(status: &str) -> bool {
    status.starts_with("install ok installed")
}

pub fn is_config_files_status(status: &str) -> bool {
    status.starts_with("deinstall ok config-files")
}

pub fn is_tracked_status(status: &str) -> bool {
    is_installed_status(status) || is_config_files_status(status)
}

fn parse_stanza(stanza: &str) -> Result<BTreeMap<String, String>> {
    let mut fields = BTreeMap::new();
    let mut current_key: Option<String> = None;

    for line in stanza.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            let key = current_key.as_ref().ok_or_else(|| {
                Error::InvalidControl("continuation without key".into())
            })?;
            let entry: &mut String = fields.get_mut(key).unwrap();
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

        current_key = Some(key.trim().to_string());
        fields.insert(key.trim().to_string(), value.trim().to_string());
    }

    Ok(fields)
}

fn format_stanza(fields: &BTreeMap<String, String>) -> String {
    let order = [
        "Package",
        "Status",
        "Priority",
        "Section",
        "Installed-Size",
        "Maintainer",
        "Architecture",
        "Source",
        "Version",
        "Replaces",
        "Provides",
        "Depends",
        "Pre-Depends",
        "Recommends",
        "Suggests",
        "Conflicts",
        "Conffiles",
        "Description",
    ];

    let mut lines = Vec::new();
    let mut written = std::collections::BTreeSet::new();

    for key in order {
        if let Some(value) = fields.get(key) {
            append_field(&mut lines, key, value);
            written.insert(key);
        }
    }

    for (key, value) in fields {
        if !written.contains(key.as_str()) {
            append_field(&mut lines, key, value);
        }
    }

    lines.join("\n")
}

fn append_field(lines: &mut Vec<String>, key: &str, value: &str) {
    // dpkg requires every Conffiles entry on a continuation line beginning with a space.
    if key == "Conffiles" {
        lines.push(format!("{key}:"));
        for line in value.lines() {
            let line = line.trim();
            if !line.is_empty() {
                lines.push(format!(" {line}"));
            }
        }
        return;
    }

    if !value.contains('\n') {
        lines.push(format!("{key}: {value}"));
        return;
    }

    let mut parts = value.split('\n');
    lines.push(format!("{key}: {}", parts.next().unwrap_or_default()));
    for part in parts {
        lines.push(format!(" {part}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_package_stanza() {
        let content = "Package: hello\nStatus: install ok installed\nVersion: 1.0\nArchitecture: all\nMaintainer: Demo\nDescription: test package\n\n";
        let stanzas = parse_status_file(content).unwrap();
        let hello = stanzas.get("hello").unwrap();
        assert_eq!(hello.get("Version").map(String::as_str), Some("1.0"));
        assert!(is_installed_status(hello.get("Status").unwrap()));
    }

    #[test]
    fn round_trips_stanza_formatting() {
        let mut fields = BTreeMap::new();
        fields.insert("Package".into(), "demo".into());
        fields.insert("Status".into(), "install ok installed".into());
        fields.insert("Version".into(), "2.0".into());
        fields.insert("Architecture".into(), "amd64".into());
        fields.insert("Description".into(), "line one\nline two".into());

        let formatted = format_stanza(&fields);
        let parsed = parse_stanza(&formatted).unwrap();
        assert_eq!(parsed.get("Package").map(String::as_str), Some("demo"));
        assert_eq!(
            parsed.get("Description").map(String::as_str),
            Some("line one\nline two")
        );
    }

    #[test]
    fn round_trips_conffiles_with_leading_space_lines() {
        let content = "\
Package: adduser
Status: install ok installed
Version: 3.137ubuntu1
Architecture: all
Conffiles:
 /etc/adduser.conf deadbeef0123456789deadbeef0123456789
 /etc/deluser.conf cafebabe0123456789cafebabe0123456789
Description: add and remove users and groups
";
        let stanzas = parse_status_file(&format!("{content}\n")).unwrap();
        let adduser = stanzas.get("adduser").unwrap();
        let formatted = format_stanza(adduser);
        assert!(formatted.contains("Conffiles:\n /etc/adduser.conf"));
        assert!(!formatted.contains("Conffiles: /etc/"));
        let reparsed = parse_stanza(&formatted).unwrap();
        assert_eq!(
            reparsed.get("Conffiles").map(String::as_str),
            adduser.get("Conffiles").map(String::as_str)
        );
    }
}
