use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use md5::{Digest as Md5Digest, Md5};
use sha2::Sha256;

use crate::acquire::DIRECT_DEB_PRIORITY;
use crate::control::ControlFile;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct PackageIndexEntry {
    pub control: ControlFile,
    pub file_path: PathBuf,
    /// Repository base URI for remote packages (`Filename` is relative to this).
    pub source_uri: Option<String>,
    /// Cached `Packages` file this entry was loaded from (remote repos).
    pub packages_index_path: Option<PathBuf>,
    /// `signed-by` keyring path from `sources.list`.
    pub signed_by: Option<String>,
    pub suite: Option<String>,
    pub component: Option<String>,
    /// Repository pin priority (higher wins when package versions tie).
    pub repo_priority: i32,
}

#[derive(Debug, Default)]
pub struct PackageIndex {
    pub packages: HashMap<String, Vec<PackageIndexEntry>>,
}

impl PackageIndex {
    pub fn load(path: &Path) -> Result<Self> {
        let base_dir = path.parent().unwrap_or(Path::new("."));
        let repo_root = find_repo_root(path);
        let content = if path.extension().is_some_and(|e| e == "gz") {
            let file = File::open(path)?;
            let mut decoder = GzDecoder::new(file);
            let mut content = String::new();
            decoder.read_to_string(&mut content)?;
            content
        } else {
            std::fs::read_to_string(path)?
        };
        Self::parse_with_root(&content, base_dir, repo_root.as_deref())
    }

    pub fn parse(content: &str, base_dir: &Path) -> Result<Self> {
        Self::parse_with_root(content, base_dir, None)
    }

    fn parse_with_root(content: &str, base_dir: &Path, repo_root: Option<&Path>) -> Result<Self> {
        let mut index = PackageIndex::default();
        let mut blocks = Vec::new();
        let mut current = String::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                if !current.is_empty() {
                    blocks.push(std::mem::take(&mut current));
                }
            } else {
                if !current.is_empty() {
                    current.push('\n');
                }
                current.push_str(line);
            }
        }
        if !current.is_empty() {
            blocks.push(current);
        }

        for block in blocks {
            let control = ControlFile::parse(&block)?;
            let file_path = if control.filename.is_empty() {
                if let Some(root) = repo_root {
                    root.join(control.full_name())
                } else {
                    base_dir.join(control.full_name())
                }
            } else if let Some(root) = repo_root {
                root.join(&control.filename)
            } else {
                base_dir.join(&control.filename)
            };
            index
                .packages
                .entry(control.package.clone())
                .or_default()
                .push(PackageIndexEntry {
                    control,
                    file_path,
                    source_uri: None,
                    packages_index_path: None,
                    signed_by: None,
                    suite: None,
                    component: None,
                    repo_priority: 500,
                });
        }

        Ok(index)
    }

    pub fn merge(&mut self, other: PackageIndex) {
        for (name, entries) in other.packages {
            self.packages.entry(name).or_default().extend(entries);
        }
    }

    pub fn search(&self, pattern: &str) -> Vec<&PackageIndexEntry> {
        let pattern = pattern.to_ascii_lowercase();
        let mut results = Vec::new();
        for (name, entries) in &self.packages {
            if name.contains(&pattern) {
                results.extend(entries.iter());
                continue;
            }
            for entry in entries {
                if entry.control.description.to_ascii_lowercase().contains(&pattern) {
                    results.push(entry);
                }
            }
        }
        results.sort_by_key(|e| e.control.package.clone());
        results
    }

    pub fn get(&self, name: &str) -> Option<&PackageIndexEntry> {
        self.packages
            .get(name)
            .and_then(|entries| entries.first())
    }

    pub fn get_best(&self, name: &str, arch: &str) -> Option<&PackageIndexEntry> {
        self.packages.get(name).and_then(|entries| {
            entries
                .iter()
                .filter(|e| {
                    e.repo_priority == DIRECT_DEB_PRIORITY
                        || e.control.architecture.is_empty()
                        || e.control.architecture == arch
                        || e.control.architecture == "all"
                        || arch == "all"
                })
                .max_by(|a, b| {
                    let version_cmp = crate::dependency::deb_version_compare(
                        &a.control.version,
                        &b.control.version,
                    );
                    if version_cmp != std::cmp::Ordering::Equal {
                        return version_cmp;
                    }
                    a.repo_priority.cmp(&b.repo_priority)
                })
        })
    }

    pub fn to_packages_file(&self) -> String {
        let mut blocks = Vec::new();
        let mut names: Vec<_> = self.packages.keys().cloned().collect();
        names.sort();
        for name in names {
            let entries = &self.packages[&name];
            for entry in entries {
                blocks.push(format_index_entry(&entry.control));
            }
        }
        blocks.join("\n\n")
    }
}

#[derive(Debug)]
pub struct Repository {
    pub root: PathBuf,
    pub index: PackageIndex,
}

#[derive(Debug, Clone)]
pub struct IndexSourceMeta {
    pub source_uri: Option<String>,
    pub signed_by: Option<String>,
    pub suite: Option<String>,
    pub component: Option<String>,
    pub priority: i32,
}

impl Repository {
    pub fn open(root: &Path) -> Result<Self> {
        let mut index = PackageIndex::default();
        for path in find_all_packages_indexes(root)? {
            index.merge(PackageIndex::load(&path)?);
        }
        Ok(Self {
            root: root.to_path_buf(),
            index,
        })
    }

    pub fn load_indexes(paths: &[PathBuf]) -> Result<PackageIndex> {
        Self::load_indexes_tagged(paths, None)
    }

    pub fn load_indexes_tagged(paths: &[PathBuf], source_uri: Option<&str>) -> Result<PackageIndex> {
        let meta = source_uri.map(|uri| IndexSourceMeta {
            source_uri: Some(uri.to_string()),
            signed_by: None,
            suite: None,
            component: None,
            priority: 500,
        });
        Self::load_indexes_with_meta(paths, meta.as_ref())
    }

    pub fn load_indexes_with_meta(
        paths: &[PathBuf],
        meta: Option<&IndexSourceMeta>,
    ) -> Result<PackageIndex> {
        let mut merged = PackageIndex::default();
        for path in paths {
            if path.exists() {
                let mut index = PackageIndex::load(path)?;
                if let Some(meta) = meta {
                    tag_index_entries(&mut index, meta, path);
                }
                merged.merge(index);
            }
        }
        Ok(merged)
    }
}

fn tag_index_entries(index: &mut PackageIndex, meta: &IndexSourceMeta, packages_path: &Path) {
    for entries in index.packages.values_mut() {
        for entry in entries.iter_mut() {
            if let Some(uri) = &meta.source_uri {
                entry.source_uri = Some(uri.clone());
            }
            if let Some(keyring) = &meta.signed_by {
                entry.signed_by = Some(keyring.clone());
            }
            if let Some(suite) = &meta.suite {
                entry.suite = Some(suite.clone());
            }
            if let Some(component) = &meta.component {
                entry.component = Some(component.clone());
            }
            entry.repo_priority = meta.priority;
            entry.packages_index_path = Some(packages_path.to_path_buf());
        }
    }
}

pub fn write_packages_index(path: &Path, index: &PackageIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = index.to_packages_file();
    if path.extension().is_some_and(|e| e == "gz") {
        let file = File::create(path)?;
        let mut enc = GzEncoder::new(file, Compression::default());
        enc.write_all(content.as_bytes())?;
        enc.finish()?;
    } else {
        std::fs::write(path, content)?;
    }
    Ok(())
}

pub fn write_release_file(path: &Path, suite: &str, codename: &str, components: &[(&str, &Path)]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut lines = vec![
        format!("Origin: Raptor"),
        format!("Label: Raptor Repository"),
        format!("Suite: {suite}"),
        format!("Codename: {codename}"),
        format!("Date: {}", chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S %z")),
        format!("Architectures: amd64 arm64 all"),
        format!("Components: {}", components.iter().map(|(c, _)| *c).collect::<Vec<_>>().join(" ")),
        String::new(),
    ];

    for (component, arch_path) in components {
        for arch in ["amd64", "arm64", "all"] {
            let packages_rel = format!("dists/{suite}/{component}/binary-{arch}/Packages");
            let packages_gz = arch_path.join(format!("Packages.gz"));
            let packages_plain = arch_path.join("Packages");
            let target = if packages_gz.exists() {
                packages_gz
            } else if packages_plain.exists() {
                packages_plain
            } else {
                continue;
            };
            let (size, md5, sha256) = file_hashes(&target)?;
            lines.push(format!("MD5Sum:"));
            lines.push(format!(" {size} {md5} {packages_rel}.gz"));
            lines.push(format!("SHA256:"));
            lines.push(format!(" {size} {sha256} {packages_rel}.gz"));
        }
        let _ = component;
    }

    std::fs::write(path, lines.join("\n"))?;
    Ok(())
}

pub fn scan_pool_directory(pool: &Path, arch: &str) -> Result<PackageIndex> {
    let mut index = PackageIndex::default();
    if !pool.exists() {
        return Ok(index);
    }

    for entry in walkdir::WalkDir::new(pool)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "deb"))
    {
        let path = entry.path();
        let archive = crate::deb::read_deb(path)?;
        let mut control = archive.control;
        control.architecture = if control.architecture.is_empty() {
            arch.to_string()
        } else {
            control.architecture.clone()
        };
        let rel = path
            .strip_prefix(pool.parent().unwrap_or(pool))
            .unwrap_or(path);
        control.filename = rel.to_string_lossy().into_owned();
        let meta = std::fs::metadata(path)?;
        control.size = meta.len().to_string();
        let bytes = std::fs::read(path)?;
        control.md5sum = format!("{:x}", Md5::digest(&bytes));
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        control.sha256 = format!("{:x}", hasher.finalize());

        index
            .packages
            .entry(control.package.clone())
            .or_default()
            .push(PackageIndexEntry {
                control,
                file_path: path.to_path_buf(),
                source_uri: None,
                packages_index_path: None,
                signed_by: None,
                suite: None,
                component: None,
                repo_priority: 500,
            });
    }

    Ok(index)
}

fn format_index_entry(control: &ControlFile) -> String {
    let mut lines = vec![
        format!("Package: {}", control.package),
        format!("Version: {}", control.version),
        format!("Architecture: {}", control.architecture),
    ];
    if !control.maintainer.is_empty() {
        lines.push(format!("Maintainer: {}", control.maintainer));
    }
    if !control.depends.is_empty() {
        lines.push(format!("Depends: {}", control.depends));
    }
    if !control.filename.is_empty() {
        lines.push(format!("Filename: {}", control.filename));
    }
    if !control.size.is_empty() {
        lines.push(format!("Size: {}", control.size));
    }
    if !control.md5sum.is_empty() {
        lines.push(format!("MD5sum: {}", control.md5sum));
    }
    if !control.sha256.is_empty() {
        lines.push(format!("SHA256: {}", control.sha256));
    }
    if !control.description.is_empty() {
        lines.push(format!("Description: {}", control.description));
    }
    lines.join("\n")
}

fn find_repo_root(packages_path: &Path) -> Option<PathBuf> {
    packages_path
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "dists"))
        .and_then(|dists| dists.parent())
        .map(Path::to_path_buf)
}

fn find_all_packages_indexes(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy();
        if name == "Packages" {
            paths.push(path.to_path_buf());
        } else if name == "Packages.gz" {
            let plain = path.with_file_name("Packages");
            if !plain.exists() {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn file_hashes(path: &Path) -> Result<(u64, String, String)> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok((
        bytes.len() as u64,
        format!("{:x}", Md5::digest(&bytes)),
        format!("{:x}", hasher.finalize()),
    ))
}

pub fn read_packages_stream<R: Read>(reader: R, base_dir: &Path) -> Result<PackageIndex> {
    let reader = BufReader::new(reader);
    let mut content = String::new();
    for line in reader.lines() {
        content.push_str(&line?);
        content.push('\n');
    }
    PackageIndex::parse(&content, base_dir)
}
