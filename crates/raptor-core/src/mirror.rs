use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::acquire::verify_control_checksums;
use crate::config::{load_yaml_file, save_yaml_file};
use crate::error::{Error, Result};
use crate::remote::fetch_url;
use crate::release::ReleaseIndex;
use crate::repository::PackageIndex;
use crate::verify::{
    parse_release_file, verify_and_extract_inrelease, verify_detached_release, verify_payload_checksums,
};

/// APT mirror configuration (`mirror.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorConfig {
    pub upstream: String,
    pub suite: String,
    #[serde(default = "default_components")]
    pub components: Vec<String>,
    #[serde(default = "default_architectures")]
    pub architectures: Vec<String>,
    #[serde(default)]
    pub keyring: Option<String>,
    #[serde(default = "default_true")]
    pub sync_indexes: bool,
    #[serde(default)]
    pub sync_pool: bool,
    #[serde(default = "default_pool_limit")]
    pub pool_package_limit: u32,
}

#[derive(Debug, Default)]
pub struct MirrorSyncReport {
    pub indexes: Vec<PathBuf>,
    pub pool: Vec<PathBuf>,
}

fn default_components() -> Vec<String> {
    vec!["main".into()]
}

fn default_architectures() -> Vec<String> {
    vec!["amd64".into(), "all".into()]
}

fn default_true() -> bool {
    true
}

fn default_pool_limit() -> u32 {
    100
}

impl MirrorConfig {
    pub const FILE_NAME: &'static str = "mirror.yaml";

    pub fn load(path: &Path) -> Result<Self> {
        load_yaml_file(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_yaml_file(path, self)
    }

    pub fn ubuntu(upstream: &str, suite: &str) -> Self {
        Self {
            upstream: upstream.into(),
            suite: suite.into(),
            components: vec![
                "main".into(),
                "restricted".into(),
                "universe".into(),
                "multiverse".into(),
            ],
            architectures: vec!["amd64".into(), "arm64".into()],
            keyring: Some("/usr/share/keyrings/ubuntu-archive-keyring.gpg".into()),
            sync_indexes: true,
            sync_pool: false,
            pool_package_limit: default_pool_limit(),
        }
    }

    pub fn mock_local(upstream_dir: &Path, suite: &str) -> Self {
        Self {
            upstream: format!("file://{}", upstream_dir.display()),
            suite: suite.into(),
            components: vec!["main".into()],
            architectures: vec!["all".into()],
            keyring: None,
            sync_indexes: true,
            sync_pool: true,
            pool_package_limit: 100,
        }
    }
}

pub fn scaffold_mirror(root: &Path, config: &MirrorConfig) -> Result<()> {
    fs::create_dir_all(root.join("dists"))?;
    fs::create_dir_all(root.join("pool"))?;
    config.save(&root.join(MirrorConfig::FILE_NAME))?;

    let sources = format!(
        "# Raptor mirror — add to /etc/apt/sources.list.d/raptor-mirror.list\n\
         deb [signed-by={}] file:{} {} {}\n",
        config.keyring.as_deref().unwrap_or("/etc/apt/keyrings/ubuntu-archive-keyring.gpg"),
        root.display(),
        config.suite,
        config.components.join(" ")
    );
    fs::write(root.join("sources.list.snippet"), sources)?;

    let readme = format!(
        "# Raptor APT Mirror\n\n\
         Upstream: {}\n\
         Suite: {}\n\n\
         Sync:\n\
           raptor repo sync --root {}\n",
        config.upstream,
        config.suite,
        root.display()
    );
    fs::write(root.join("README.md"), readme)?;
    Ok(())
}

/// Sync indexes and optionally pool packages from upstream into a local mirror.
pub fn sync_mirror(root: &Path, config: &MirrorConfig) -> Result<MirrorSyncReport> {
    let indexes = if config.sync_indexes {
        sync_mirror_indexes(root, config)?
    } else {
        Vec::new()
    };
    let pool = if config.sync_pool {
        sync_mirror_pool(root, config)?
    } else {
        Vec::new()
    };
    Ok(MirrorSyncReport { indexes, pool })
}

pub fn sync_mirror_indexes(root: &Path, config: &MirrorConfig) -> Result<Vec<PathBuf>> {
    let keyring = config.keyring.as_deref().map(Path::new);
    let suite_base = format!(
        "{}/dists/{}/",
        config.upstream.trim_end_matches('/'),
        config.suite
    );
    let dest_base = root.join(format!("dists/{}", config.suite));
    fs::create_dir_all(&dest_base)?;

    let release_index = if let Some(keyring) = keyring {
        fetch_release_index(&suite_base, keyring)?
    } else {
        let release_path = dest_base.join("Release");
        let temp = fetch_url(&format!("{suite_base}Release"))?;
        fs::copy(&temp, &release_path)?;
        let _ = fs::remove_file(&temp);
        parse_release_file(&release_path)?
    };

    let mut synced = Vec::new();
    for component in &config.components {
        for arch in &config.architectures {
            let rel_paths = [
                format!("{component}/binary-{arch}/Packages.gz"),
                format!("{component}/binary-{arch}/Packages"),
            ];
            let out_dir = dest_base.join(format!("{component}/binary-{arch}"));
            fs::create_dir_all(&out_dir)?;

            for rel_path in &rel_paths {
                let Some(checksum) = release_index.checksum(rel_path) else {
                    continue;
                };
                let url = format!("{suite_base}{rel_path}");
                let temp = fetch_url(&url)?;
                verify_payload_checksums(&temp, checksum)?;
                let dest = if rel_path.ends_with(".gz") {
                    let plain = out_dir.join("Packages");
                    decompress_gz(&temp, &plain)?;
                    let _ = fs::remove_file(&temp);
                    plain
                } else {
                    let plain = out_dir.join("Packages");
                    fs::rename(&temp, &plain)?;
                    plain
                };
                synced.push(dest);
                break;
            }
        }
    }

    Ok(synced)
}

pub fn sync_mirror_pool(root: &Path, config: &MirrorConfig) -> Result<Vec<PathBuf>> {
    let upstream = config.upstream.trim_end_matches('/');
    let dest_pool = root.join("pool");
    fs::create_dir_all(&dest_pool)?;

    let mut synced = Vec::new();
    let mut count = 0u32;

    for component in &config.components {
        for arch in &config.architectures {
            let packages_path = root.join(format!(
                "dists/{}/{}/binary-{}/Packages",
                config.suite, component, arch
            ));
            if !packages_path.exists() {
                continue;
            }

            let index = PackageIndex::load(&packages_path)?;
            let mut entries: Vec<_> = index
                .packages
                .values()
                .flat_map(|v| v.iter())
                .collect();
            entries.sort_by_key(|e| e.control.filename.clone());

            for entry in entries {
                if count >= config.pool_package_limit {
                    return Ok(synced);
                }
                let filename = entry.control.filename.trim();
                if filename.is_empty() {
                    continue;
                }
                let url = format!("{upstream}/{filename}");
                let temp = fetch_url(&url)?;
                verify_control_checksums(&temp, &entry.control).map_err(|e| {
                    let _ = fs::remove_file(&temp);
                    e
                })?;

                let dest = dest_pool.join(filename.trim_start_matches('/'));
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                if dest.exists() {
                    let _ = fs::remove_file(&dest);
                }
                fs::rename(&temp, &dest)?;
                synced.push(dest);
                count += 1;
            }
        }
    }

    Ok(synced)
}

fn fetch_release_index(suite_base: &str, keyring: &Path) -> Result<ReleaseIndex> {
    let inrelease_url = format!("{suite_base}InRelease");
    if let Ok(temp) = fetch_url(&inrelease_url) {
        if let Ok(body) = verify_and_extract_inrelease(keyring, &temp) {
            let _ = fs::remove_file(&temp);
            return ReleaseIndex::parse(&body);
        }
        let _ = fs::remove_file(&temp);
    }

    let release_temp = fetch_url(&format!("{suite_base}Release"))?;
    let sig_temp = fetch_url(&format!("{suite_base}Release.gpg"))?;
    verify_detached_release(keyring, &release_temp, &sig_temp)?;
    let index = parse_release_file(&release_temp)?;
    let _ = fs::remove_file(&release_temp);
    let _ = fs::remove_file(&sig_temp);
    Ok(index)
}

fn decompress_gz(gz_path: &Path, dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let file = fs::File::open(gz_path)?;
    let mut decoder = GzDecoder::new(file);
    let mut content = String::new();
    decoder
        .read_to_string(&mut content)
        .map_err(|e| Error::RemoteFetch(e.to_string()))?;
    fs::write(dest, content)?;
    Ok(())
}

pub mod mock;

#[cfg(test)]
mod tests {
    use super::mock::MockUpstream;
    use super::*;

    #[test]
    fn mirror_sync_round_trip_with_mock() {
        let deb = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/hello-raptor_0.1.0_all.deb");
        if !deb.exists() {
            return;
        }

        let upstream = std::env::temp_dir().join(format!("raptor-mirror-up-{}", std::process::id()));
        let mirror = std::env::temp_dir().join(format!("raptor-mirror-dn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&upstream);
        let _ = fs::remove_dir_all(&mirror);

        let mock = MockUpstream::build(&upstream, 2).unwrap();
        let config = mock.mirror_config(1);
        let report = sync_mirror(&mirror, &config).unwrap();
        assert_eq!(report.indexes.len(), 1);
        assert_eq!(report.pool.len(), 1);

        let _ = fs::remove_dir_all(&upstream);
        let _ = fs::remove_dir_all(&mirror);
    }
}
