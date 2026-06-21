use crate::config::{RaptorConfig, UnattendedConfig};
use crate::error::Result;
use crate::installer::InstallContext;
use crate::remote::fetch_remote_indexes;
use crate::repository::{IndexSourceMeta, PackageIndex, Repository};
use crate::resolver::Resolver;
use crate::sources::{load_all_sources, SourceType, SourcesList};
use crate::state::{deb_architecture, detect_architecture, State};

fn load_merged_index(sources: &SourcesList, cache_dir: &std::path::Path, arch: &str) -> Result<PackageIndex> {
    let mut index = PackageIndex::default();
    for root in sources.local_repo_roots() {
        if let Ok(repo) = Repository::open(&root) {
            index.merge(repo.index);
        }
    }

    for source in &sources.entries {
        if !source.enabled || source.source_type != SourceType::Deb {
            continue;
        }
        if !source.uri.starts_with("http://") && !source.uri.starts_with("https://") {
            continue;
        }
        for component in &source.components {
            let local = cache_dir.join(
                source
                    .uri
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .replace('/', "_"),
            )
            .join(format!(
                "dists/{}/{}/binary-{}/Packages",
                source.suite, component, arch
            ));
            if !local.exists() {
                continue;
            }
            let meta = IndexSourceMeta {
                source_uri: Some(source.uri.clone()),
                signed_by: source.signed_by.clone(),
                suite: Some(source.suite.clone()),
                component: Some(component.clone()),
                priority: source.priority,
            };
            if let Ok(cached) = Repository::load_indexes_with_meta(&[local], Some(&meta)) {
                index.merge(cached);
            }
        }
    }

    Ok(index)
}

/// Run one unattended maintenance cycle: update indexes and optionally upgrade packages.
pub fn run_unattended_cycle(config: &RaptorConfig, apply: bool) -> Result<UnattendedReport> {
    config.apply_env();
    let unattended = &config.unattended;
    if !unattended.enabled {
        return Ok(UnattendedReport {
            updated: false,
            upgraded: Vec::new(),
            skipped: "unattended upgrades disabled".into(),
        });
    }

    let sources = load_all_sources()?;
    let arch = config
        .system
        .architecture
        .clone()
        .unwrap_or_else(|| deb_architecture(&detect_architecture()));

    let mut report = UnattendedReport {
        updated: false,
        upgraded: Vec::new(),
        skipped: String::new(),
    };

    if unattended.auto_update {
        fetch_remote_indexes(&sources, &config.paths.cache, &arch)?;
        report.updated = true;
    }

    if !unattended.auto_upgrade {
        report.skipped = "auto_upgrade disabled".into();
        return Ok(report);
    }

    let state = State::load(&config.paths.state)?;
    let index = load_merged_index(&sources, &config.paths.cache, &arch)?;

    let resolver = Resolver::new(&index, &state, &arch);
    let plan = resolver.plan_upgrade()?;
    report.upgraded = plan
        .actions
        .iter()
        .filter(|a| package_allowed(&a.package, unattended))
        .map(|a| a.package.clone())
        .collect();

    if apply && !report.upgraded.is_empty() {
        let filtered = filter_plan(plan, unattended);
        let mut ctx = InstallContext {
            state,
            index,
            arch,
            install_root: config.paths.root.clone(),
            archives_dir: config.paths.archives.clone(),
        };
        ctx.apply_plan(&filtered)?;
    }

    Ok(report)
}

fn filter_plan(
    plan: crate::resolver::InstallPlan,
    cfg: &UnattendedConfig,
) -> crate::resolver::InstallPlan {
    if cfg.packages.is_empty() {
        return plan;
    }
    crate::resolver::InstallPlan {
        actions: plan
            .actions
            .into_iter()
            .filter(|a| package_allowed(&a.package, cfg))
            .collect(),
    }
}

fn package_allowed(name: &str, cfg: &UnattendedConfig) -> bool {
    if cfg.packages.is_empty() {
        return true;
    }
    cfg.packages.iter().any(|p| name == p || name.starts_with(p))
}

#[derive(Debug, Default)]
pub struct UnattendedReport {
    pub updated: bool,
    pub upgraded: Vec<String>,
    pub skipped: String,
}

pub fn daemon_loop(config: RaptorConfig, run_once: bool, apply: bool) -> Result<()> {
    let interval = std::time::Duration::from_secs(config.unattended.interval_hours * 3600);
    loop {
        match run_unattended_cycle(&config, apply) {
            Ok(report) => {
                if report.updated {
                    eprintln!("raptor daemon: package indexes updated");
                }
                if !report.upgraded.is_empty() {
                    eprintln!(
                        "raptor daemon: upgraded packages: {}",
                        report.upgraded.join(", ")
                    );
                }
                if !report.skipped.is_empty() && !config.unattended.auto_upgrade {
                    eprintln!("raptor daemon: {}", report.skipped);
                }
            }
            Err(e) => eprintln!("raptor daemon: cycle failed: {e}"),
        }

        if run_once {
            break;
        }
        std::thread::sleep(interval);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_filter_respects_allowlist() {
        let cfg = UnattendedConfig {
            packages: vec!["linux-".into()],
            ..Default::default()
        };
        assert!(package_allowed("linux-image-generic", &cfg));
        assert!(!package_allowed("hello", &cfg));
    }
}
