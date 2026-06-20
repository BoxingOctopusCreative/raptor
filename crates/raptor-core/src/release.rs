use std::collections::HashMap;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct ReleaseIndex {
    entries: HashMap<String, ReleaseChecksum>,
}

#[derive(Debug, Clone)]
pub struct ReleaseChecksum {
    pub size: u64,
    pub md5: Option<String>,
    pub sha256: Option<String>,
}

impl ReleaseIndex {
    pub fn parse(content: &str) -> Result<Self> {
        let mut index = ReleaseIndex::default();
        let mut section: Option<&str> = None;

        for line in content.lines() {
            if line.is_empty() {
                section = None;
                continue;
            }

            if !line.starts_with(' ') {
                section = Some(line.trim_end_matches(':'));
                continue;
            }

            let line = line.trim();
            let Some(section) = section else {
                continue;
            };

            let mut parts = line.split_whitespace();
            let hash = parts
                .next()
                .ok_or_else(|| Error::InvalidRelease(format!("invalid release line: {line}")))?;
            let size: u64 = parts
                .next()
                .ok_or_else(|| Error::InvalidRelease(format!("invalid release line: {line}")))?
                .parse()
                .map_err(|_| Error::InvalidRelease(format!("invalid release size: {line}")))?;
            let path = parts
                .next()
                .ok_or_else(|| Error::InvalidRelease(format!("missing release path: {line}")))?
                .to_string();

            let entry = index.entries.entry(path).or_insert_with(|| ReleaseChecksum {
                size,
                md5: None,
                sha256: None,
            });
            entry.size = size;
            match section {
                "MD5sum" => entry.md5 = Some(hash.to_ascii_lowercase()),
                "SHA256" => entry.sha256 = Some(hash.to_ascii_lowercase()),
                "SHA1" => {}
                _ => {}
            }
        }

        Ok(index)
    }

    pub fn checksum(&self, path: &str) -> Option<&ReleaseChecksum> {
        self.entries.get(path)
    }
}

/// Extract the signed payload from an OpenPGP clearsigned `InRelease` file.
pub fn extract_inrelease_body(content: &str) -> Result<String> {
    let signature_start = content
        .find("-----BEGIN PGP SIGNATURE-----")
        .ok_or_else(|| Error::InvalidRelease("missing PGP signature in InRelease".into()))?;

    let header_end = content
        .find("-----BEGIN PGP SIGNED MESSAGE-----")
        .map(|i| i + "-----BEGIN PGP SIGNED MESSAGE-----".len())
        .ok_or_else(|| Error::InvalidRelease("missing signed message header in InRelease".into()))?;

    let body_region = &content[header_end..signature_start];
    let body = body_region
        .split_once("\n\n")
        .map(|(_, body)| body)
        .unwrap_or(body_region)
        .trim_end()
        .to_string();

    if body.is_empty() {
        return Err(Error::InvalidRelease("empty InRelease payload".into()));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_checksum_sections() {
        let content = r#"Origin: Example
MD5sum:
 abcdef0123456789abcdef0123456789 123 main/binary-amd64/Packages.gz
SHA256:
 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef 123 main/binary-amd64/Packages.gz
"#;
        let index = ReleaseIndex::parse(content).unwrap();
        let entry = index
            .checksum("main/binary-amd64/Packages.gz")
            .unwrap();
        assert_eq!(entry.size, 123);
        assert_eq!(
            entry.md5.as_deref(),
            Some("abcdef0123456789abcdef0123456789")
        );
        assert_eq!(
            entry.sha256.as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn extracts_inrelease_payload() {
        let content = "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA512\n\nOrigin: Test\nSuite: jammy\n-----BEGIN PGP SIGNATURE-----\n...\n-----END PGP SIGNATURE-----\n";
        let body = extract_inrelease_body(content).unwrap();
        assert!(body.contains("Origin: Test"));
        assert!(body.contains("Suite: jammy"));
    }
}
