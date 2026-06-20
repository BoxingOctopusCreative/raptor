use regex::Regex;
use std::cmp::Ordering;

/// A single dependency alternative (e.g. `libc6 (>= 2.34)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub constraints: Vec<VersionConstraint>,
    pub arch_filter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    Equal(String),
    GreaterEqual(String),
    Greater(String),
    LessEqual(String),
    Less(String),
    NotEqual(String),
}

pub fn parse_dependency_list(input: &str) -> Vec<Dependency> {
    if input.trim().is_empty() {
        return Vec::new();
    }

    input
        .split(',')
        .flat_map(parse_alternative_group)
        .collect()
}

fn parse_alternative_group(group: &str) -> Vec<Dependency> {
    group
        .split('|')
        .map(|alt| parse_single_dependency(alt.trim()))
        .collect()
}

fn parse_single_dependency(input: &str) -> Dependency {
    let re = Regex::new(
        r"^(?P<name>[a-zA-Z0-9+.-]+)(?:\s*\[(?P<arch>[^\]]+)\])?(?:\s*\((?P<op><=|>=|<>|<<|>>|=|<|>)\s*(?P<ver>[^)]+)\))?$",
    )
    .unwrap();

    if let Some(caps) = re.captures(input) {
        let name = caps.name("name").unwrap().as_str().to_string();
        let arch_filter = caps.name("arch").map(|m| m.as_str().to_string());
        let constraints = match (caps.name("op"), caps.name("ver")) {
            (Some(op), Some(ver)) => vec![VersionConstraint::from_op(op.as_str(), ver.as_str())],
            _ => Vec::new(),
        };
        Dependency {
            name,
            constraints,
            arch_filter,
        }
    } else {
        Dependency {
            name: input.to_string(),
            constraints: Vec::new(),
            arch_filter: None,
        }
    }
}

impl VersionConstraint {
    pub fn from_op(op: &str, version: &str) -> Self {
        match op {
            "=" => VersionConstraint::Equal(version.to_string()),
            ">=" => VersionConstraint::GreaterEqual(version.to_string()),
            ">>" => VersionConstraint::Greater(version.to_string()),
            "<=" => VersionConstraint::LessEqual(version.to_string()),
            "<<" => VersionConstraint::Less(version.to_string()),
            "<" => VersionConstraint::Less(version.to_string()),
            ">" => VersionConstraint::Greater(version.to_string()),
            "<>" => VersionConstraint::NotEqual(version.to_string()),
            _ => VersionConstraint::Equal(version.to_string()),
        }
    }

    pub fn satisfies(&self, version: &str) -> bool {
        let cmp = deb_version_compare(version, self.version_ref());
        match self {
            VersionConstraint::Equal(_) => cmp == Ordering::Equal,
            VersionConstraint::GreaterEqual(_) => cmp != Ordering::Less,
            VersionConstraint::Greater(_) => cmp == Ordering::Greater,
            VersionConstraint::LessEqual(_) => cmp != Ordering::Greater,
            VersionConstraint::Less(_) => cmp == Ordering::Less,
            VersionConstraint::NotEqual(_) => cmp != Ordering::Equal,
        }
    }

    fn version_ref(&self) -> &str {
        match self {
            VersionConstraint::Equal(v)
            | VersionConstraint::GreaterEqual(v)
            | VersionConstraint::Greater(v)
            | VersionConstraint::LessEqual(v)
            | VersionConstraint::Less(v)
            | VersionConstraint::NotEqual(v) => v,
        }
    }
}

impl Dependency {
    pub fn is_satisfied_by(&self, name: &str, version: &str) -> bool {
        if self.name != name {
            return false;
        }
        if self.constraints.is_empty() {
            return true;
        }
        self.constraints.iter().all(|c| c.satisfies(version))
    }
}

/// Debian version comparison (dpkg --compare-versions algorithm).
pub fn deb_version_compare(a: &str, b: &str) -> Ordering {
    let mut a_parts = a;
    let mut b_parts = b;

    loop {
        let a_non_digit = next_non_digit(a_parts);
        let b_non_digit = next_non_digit(b_parts);

        if a_non_digit != b_non_digit {
            return a_non_digit.cmp(&b_non_digit);
        }

        a_parts = &a_parts[a_non_digit.len()..];
        b_parts = &b_parts[b_non_digit.len()..];

        let a_digit = next_digit(a_parts);
        let b_digit = next_digit(b_parts);

        if a_digit != b_digit {
            let a_trim = a_digit.trim_start_matches('0');
            let b_trim = b_digit.trim_start_matches('0');
            let a_trim = if a_trim.is_empty() { "0" } else { a_trim };
            let b_trim = if b_trim.is_empty() { "0" } else { b_trim };
            if a_trim.len() != b_trim.len() {
                return a_trim.len().cmp(&b_trim.len());
            }
            return a_trim.cmp(b_trim);
        }

        if a_parts.is_empty() && b_parts.is_empty() {
            return Ordering::Equal;
        }

        a_parts = &a_parts[a_digit.len()..];
        b_parts = &b_parts[b_digit.len()..];
    }
}

fn next_non_digit(s: &str) -> &str {
    let end = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    &s[..end]
}

fn next_digit(s: &str) -> &str {
    if s.is_empty() {
        return "";
    }
    if !s.chars().next().unwrap().is_ascii_digit() {
        return "";
    }
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert_eq!(deb_version_compare("1.0", "1.0"), Ordering::Equal);
        assert_eq!(deb_version_compare("2.0", "1.9"), Ordering::Greater);
        assert_eq!(deb_version_compare("1.10", "1.9"), Ordering::Greater);
        assert_eq!(deb_version_compare("1.0-1", "1.0"), Ordering::Greater);
    }

    #[test]
    fn parses_dependency() {
        let deps = parse_dependency_list("libc6 (>= 2.34), bash | dash");
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].name, "libc6");
        assert!(matches!(
            deps[0].constraints[0],
            VersionConstraint::GreaterEqual(_)
        ));
        assert_eq!(deps[1].name, "bash");
        assert_eq!(deps[2].name, "dash");
    }
}
