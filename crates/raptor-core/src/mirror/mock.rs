//! Local `file://` upstream for mirror sync tests (no network, minimal disk use).

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::mirror::MirrorConfig;
use crate::repository::{scan_pool_directory, write_packages_index};

pub struct MockUpstream {
    pub root: PathBuf,
    pub suite: String,
    pub component: String,
}

impl MockUpstream {
    pub fn build(root: &Path, package_count: usize) -> Result<Self> {
        let suite = "stable".into();
        let component = "main".into();
        let pool = root.join("pool");
        let deb_src = demo_deb_path();
        if !deb_src.exists() {
            return Err(crate::error::Error::Other(format!(
                "demo deb missing at {}; run examples/demo.sh first",
                deb_src.display()
            )));
        }

        for i in 0..package_count {
            let dest = pool.join("h").join(format!("hello-mirror-{i}_0.1.0_all.deb"));
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&deb_src, &dest)?;
        }

        let index = scan_pool_directory(&pool, "all")?;
        let packages_dir = root.join(format!("dists/{suite}/{component}/binary-all"));
        fs::create_dir_all(&packages_dir)?;
        let packages_path = packages_dir.join("Packages");
        write_packages_index(&packages_path, &index)?;
        write_mock_release(
            &root.join(format!("dists/{suite}/Release")),
            &packages_path,
            &format!("{component}/binary-all/Packages"),
        )?;

        Ok(Self {
            root: root.to_path_buf(),
            suite,
            component,
        })
    }

    pub fn upstream_uri(&self) -> String {
        format!("file://{}", self.root.display())
    }

    pub fn mirror_config(&self, pool_limit: u32) -> MirrorConfig {
        MirrorConfig {
            upstream: self.upstream_uri(),
            suite: self.suite.clone(),
            components: vec![self.component.clone()],
            architectures: vec!["all".into()],
            keyring: None,
            sync_indexes: true,
            sync_pool: true,
            pool_package_limit: pool_limit,
        }
    }
}

fn demo_deb_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/hello-raptor_0.1.0_all.deb")
}

fn write_mock_release(release_path: &Path, packages_path: &Path, rel_path: &str) -> Result<()> {
    let bytes = fs::read(packages_path)?;
    let size = bytes.len() as u64;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());
    let content = format!(
        "Origin: Raptor Mock\nSuite: stable\nCodename: stable\n\nSHA256:\n {sha256} {size} {rel_path}\n"
    );
    if let Some(parent) = release_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(release_path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_file_uri_upstream() {
        if !demo_deb_path().exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("raptor-mock-build-{}", std::process::id()));
        let mock = MockUpstream::build(&dir, 1).unwrap();
        assert!(mock.upstream_uri().starts_with("file://"));
        let _ = fs::remove_dir_all(dir);
    }
}
