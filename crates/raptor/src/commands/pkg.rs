use std::fs;
use std::path::{Path, PathBuf};

use raptor_core::control::ControlFile;
use raptor_core::deb::{build_deb_from_directory, deb_path_for, read_deb};
use raptor_core::manifest::PackageManifest;
use walkdir::WalkDir;

use crate::commands::core::{cmd_info, cmd_install, cmd_list, cmd_remove, cmd_search};
use crate::commands::repo::cmd_repo_index;
use crate::global::GlobalOpts;

pub fn cmd_pkg_get(packages: Vec<String>, global: &GlobalOpts) -> anyhow::Result<()> {
    cmd_install(packages, global)
}

pub fn cmd_pkg_remove(
    packages: Vec<String>,
    purge: bool,
    global: &GlobalOpts,
) -> anyhow::Result<()> {
    cmd_remove(packages, purge, global)
}

pub fn cmd_pkg_search(pattern: String) -> anyhow::Result<()> {
    cmd_search(pattern)
}

pub fn cmd_pkg_info(package: String) -> anyhow::Result<()> {
    cmd_info(package)
}

pub fn cmd_pkg_list() -> anyhow::Result<()> {
    cmd_list()
}

pub fn cmd_pkg_init(name: &str, version: &str, arch: &str) -> anyhow::Result<()> {
    PackageManifest::write_default(Path::new("raptor.yaml"), name, version, arch)?;
    fs::create_dir_all("data")?;
    println!("Created raptor.yaml and data/");
    Ok(())
}

pub fn cmd_pkg_build(
    manifest: PathBuf,
    root: Option<PathBuf>,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    if let Some(root) = root {
        let output = output.ok_or_else(|| anyhow::anyhow!("--output is required with --root"))?;
        return build_from_debian_tree(&root, &output);
    }
    build_from_manifest(&manifest, output)
}

pub fn cmd_pkg_publish(
    deb: PathBuf,
    repo: PathBuf,
    suite: String,
    component: String,
    arch: String,
) -> anyhow::Result<()> {
    let pool = repo.join("pool");
    fs::create_dir_all(&pool)?;

    let archive = read_deb(&deb)?;
    let dest = deb_path_for(&archive.control, &pool);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&deb, &dest)?;
    println!(
        "Added {} {} to pool",
        archive.control.package, archive.control.version
    );

    cmd_repo_index(
        repo,
        suite.clone(),
        suite,
        component,
        arch,
    )
}

fn build_from_manifest(manifest_path: &Path, output: Option<PathBuf>) -> anyhow::Result<()> {
    if !manifest_path.exists() {
        if manifest_path.file_name() == Some("raptor.yaml".as_ref())
            && PathBuf::from("raptor.toml").exists()
        {
            anyhow::bail!(
                "found legacy raptor.toml; rename to raptor.yaml or pass --manifest raptor.toml after converting to YAML"
            );
        }
    }
    let manifest = PackageManifest::load(manifest_path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", manifest_path.display()))?;

    let control = ControlFile {
        package: manifest.package.name,
        version: manifest.package.version,
        architecture: manifest.package.architecture,
        maintainer: manifest.package.maintainer,
        description: manifest.package.description,
        depends: manifest.package.depends,
        section: manifest.package.section,
        priority: manifest.package.priority,
        ..Default::default()
    };

    let data_source = manifest
        .data
        .as_ref()
        .map(|d| d.source.clone())
        .unwrap_or_else(|| "data".into());
    let data_dir = PathBuf::from(&data_source);
    if !data_dir.exists() {
        anyhow::bail!("data directory '{data_source}' does not exist");
    }

    let staging = std::env::temp_dir().join(format!("raptor-pkg-{}", control.package));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let dest_prefix = manifest
        .data
        .as_ref()
        .map(|d| d.dest_prefix.trim_start_matches('/').to_string())
        .unwrap_or_default();

    let install_root = if dest_prefix.is_empty() {
        staging.clone()
    } else {
        staging.join(&dest_prefix)
    };
    fs::create_dir_all(&install_root)?;

    for pattern in &manifest.files {
        copy_glob(pattern, &install_root)?;
    }

    if manifest.files.is_empty() {
        copy_tree(&data_dir, &install_root)?;
    }

    let output = output.unwrap_or_else(|| PathBuf::from("target").join(control.full_name()));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    build_deb_from_directory(&output, &control, &staging, None)?;
    println!("Built {}", output.display());
    Ok(())
}

fn build_from_debian_tree(root: &Path, output: &Path) -> anyhow::Result<()> {
    let debian_dir = root.join("DEBIAN");
    let control_path = debian_dir.join("control");
    if !control_path.exists() {
        anyhow::bail!("missing DEBIAN/control in {}", root.display());
    }

    let control = ControlFile::from_path(&control_path)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    build_deb_from_directory(output, &control, root, Some(&debian_dir))?;
    println!("Built {}", output.display());
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    for entry in WalkDir::new(src).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        let rel = path.strip_prefix(src)?;
        let target = dest.join(rel);
        if path.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &target)?;
        }
    }
    Ok(())
}

fn copy_glob(pattern: &str, dest: &Path) -> anyhow::Result<()> {
    let paths = glob_paths(pattern)?;
    for path in paths {
        let rel = path.strip_prefix(".").unwrap_or(&path);
        let target = dest.join(rel);
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn glob_paths(pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if pattern.contains('*') {
        let (base, rest) = pattern.split_once('*').unwrap_or((pattern, ""));
        let base_path = PathBuf::from(base.trim_end_matches('/'));
        if base_path.is_dir() {
            for entry in WalkDir::new(&base_path).into_iter().filter_map(Result::ok) {
                let p = entry.path();
                if rest.is_empty() || p.to_string_lossy().contains(rest.trim_start_matches('/')) {
                    results.push(p.to_path_buf());
                }
            }
        }
    } else {
        results.push(PathBuf::from(pattern));
    }
    Ok(results)
}
