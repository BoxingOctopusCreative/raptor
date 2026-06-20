use std::fs;
use std::path::PathBuf;

use raptor_core::mirror::{scaffold_mirror, sync_mirror, MirrorConfig};
use raptor_core::ppa::{
    add_ppa, is_ppa_source, keyrings_dir_path, list_ppas, remove_ppa, sources_list_d_path,
};
use raptor_core::repo_config::RepoConfig;
use raptor_core::repository::{scan_pool_directory, write_packages_index, write_release_file};
use raptor_core::scaffold::{scaffold_ppa_repo, scaffold_private_repo};
use raptor_core::sources::{load_all_sources, SourceType};

use crate::commands::core::cmd_update;
use crate::context::Context;

pub fn cmd_repo_update() -> anyhow::Result<()> {
    cmd_update()
}

pub fn cmd_repo_priority(packages: Vec<String>) -> anyhow::Result<()> {
    if packages.is_empty() {
        anyhow::bail!("no packages specified");
    }

    let ctx = Context::load()?;
    for package in packages {
        println!("{package}:");
        if let Some(installed) = ctx.state.get(&package) {
            println!("  Installed: {}/{}", installed.version, installed.architecture);
        } else {
            println!("  Installed: (none)");
        }

        let mut candidates: Vec<_> = ctx
            .index
            .search(&package)
            .into_iter()
            .filter(|e| e.control.package == package)
            .collect();
        candidates.sort_by(|a, b| b.control.version.cmp(&a.control.version));

        if candidates.is_empty() {
            println!("  Candidate: (none)");
            continue;
        }

        for entry in candidates {
            let origin = entry
                .source_uri
                .as_deref()
                .unwrap_or("local");
            println!(
                "  Candidate: {} {} from {}",
                entry.control.version, entry.control.architecture, origin
            );
        }
    }
    Ok(())
}

pub fn cmd_repo_add(
    uri: String,
    suite: String,
    component: String,
    signed_by: Option<PathBuf>,
) -> anyhow::Result<()> {
    let list_d = sources_list_d_path();
    fs::create_dir_all(&list_d)?;

    let filename = repo_list_filename(&uri);
    let line = if let Some(ref key) = signed_by {
        format!(
            "deb [signed-by={}] {} {} {}\n",
            key.display(),
            uri,
            suite,
            component
        )
    } else {
        format!("deb {} {} {}\n", uri, suite, component)
    };

    let path = list_d.join(filename);
    fs::write(&path, line)?;
    println!("Added repository: {}", path.display());
    Ok(())
}

pub fn cmd_repo_add_ppa(
    ppa: String,
    suite: Option<String>,
    skip_key: bool,
) -> anyhow::Result<()> {
    let config = add_ppa(
        &ppa,
        suite.as_deref(),
        &sources_list_d_path(),
        &keyrings_dir_path(),
        skip_key,
    )?;
    println!(
        "PPA added: ppa:{}/{} ({})",
        config.ppa.owner, config.ppa.name, config.suite
    );
    println!(
        "Source file: {}",
        sources_list_d_path()
            .join(&config.list_filename)
            .display()
    );
    Ok(())
}

pub fn cmd_repo_remove_ppa(ppa: String, suite: Option<String>) -> anyhow::Result<()> {
    remove_ppa(
        &ppa,
        suite.as_deref(),
        &sources_list_d_path(),
        &keyrings_dir_path(),
    )?;
    let ppa_ref = raptor_core::parse_ppa(&ppa)?;
    println!("PPA removed: ppa:{}/{}", ppa_ref.owner, ppa_ref.name);
    Ok(())
}

pub fn cmd_repo_list() -> anyhow::Result<()> {
    let list_d = sources_list_d_path();
    let ppas = list_ppas(&list_d)?;
    let ppa_uris: std::collections::HashSet<String> =
        ppas.iter().map(|p| p.uri.clone()).collect();

    if ppas.is_empty() && !list_d.is_dir() {
        println!("No repositories configured.");
        return Ok(());
    }

    for ppa in &ppas {
        println!(
            "[PPA] ppa:{}/{} ({}) -> {}",
            ppa.ppa.owner, ppa.ppa.name, ppa.suite, ppa.uri
        );
    }

    let sources = load_all_sources()?;
    for entry in &sources.entries {
        if !entry.enabled || entry.source_type != SourceType::Deb {
            continue;
        }
        if ppa_uris.contains(&entry.uri) || is_ppa_source(entry) {
            continue;
        }
        let components = entry.components.join(" ");
        let signed = entry
            .signed_by
            .as_deref()
            .map(|k| format!(" [signed-by={k}]"))
            .unwrap_or_default();
        println!(
            "[Repository] {} {}{}{}",
            entry.uri, entry.suite, components, signed
        );
    }

    if ppas.is_empty() && sources.entries.is_empty() {
        println!("No repositories configured.");
    }
    Ok(())
}

pub fn cmd_repo_create(
    kind: RepoCreateKind,
    root: PathBuf,
    suite: String,
    component: String,
    owner: Option<String>,
    name: Option<String>,
    upstream: Option<String>,
) -> anyhow::Result<()> {
    match kind {
        RepoCreateKind::Private => {
            let config = scaffold_private_repo(&root, &suite, &component)?;
            println!("Created private repository at {}", root.display());
            println!(
                "Config: {}/{}",
                root.display(),
                RepoConfig::FILE_NAME
            );
            println!(
                "Suite: {}  Component: {}",
                config.suite,
                config.components.join(", ")
            );
        }
        RepoCreateKind::Ppa => {
            let owner = owner.ok_or_else(|| anyhow::anyhow!("--owner is required for PPA repos"))?;
            let name = name.ok_or_else(|| anyhow::anyhow!("--name is required for PPA repos"))?;
            let config = scaffold_ppa_repo(&root, &owner, &name, &suite)?;
            println!("Created PPA repository at {}", root.display());
            if let Some(ppa) = &config.ppa {
                println!("PPA: {}/{}", ppa.owner, ppa.name);
            }
        }
        RepoCreateKind::Mirror => {
            let upstream = upstream.unwrap_or_else(|| "http://archive.ubuntu.com/ubuntu".into());
            let config = MirrorConfig::ubuntu(&upstream, &suite);
            scaffold_mirror(&root, &config)?;
            println!("Created mirror at {}", root.display());
            println!("Upstream: {upstream}  Suite: {suite}");
            println!("Sync with: raptor repo sync --root {}", root.display());
        }
    }
    Ok(())
}

pub fn cmd_repo_index(
    repo: PathBuf,
    suite: String,
    codename: String,
    component: String,
    arch: String,
) -> anyhow::Result<()> {
    let pool = repo.join("pool");
    let index = scan_pool_directory(&pool, &arch)?;

    let packages_dir = repo.join(format!(
        "dists/{}/{}/binary-{}/",
        suite, component, arch
    ));
    fs::create_dir_all(&packages_dir)?;

    write_packages_index(&packages_dir.join("Packages"), &index)?;
    write_packages_index(&packages_dir.join("Packages.gz"), &index)?;

    let release_path = repo.join(format!("dists/{suite}/Release"));
    write_release_file(
        &release_path,
        &suite,
        &codename,
        &[(&component, &packages_dir)],
    )?;

    println!(
        "Indexed {} packages -> {}",
        index.packages.len(),
        packages_dir.display()
    );
    Ok(())
}

pub fn cmd_repo_sync(root: PathBuf) -> anyhow::Result<()> {
    let config = MirrorConfig::load(&root.join(MirrorConfig::FILE_NAME))?;
    let report = sync_mirror(&root, &config)?;
    println!(
        "Synced {} package indexes and {} pool packages",
        report.indexes.len(),
        report.pool.len()
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RepoCreateKind {
    Private,
    Ppa,
    Mirror,
}

fn repo_list_filename(uri: &str) -> String {
    let slug = uri
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("file:")
        .replace(['/', ':', '.'], "-")
        .trim_matches('-')
        .to_string();
    format!("raptor-{slug}.list")
}
