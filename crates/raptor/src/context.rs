use std::path::PathBuf;

use raptor_core::config::RaptorConfig;
use raptor_core::repository::{IndexSourceMeta, PackageIndex, Repository};
use raptor_core::sources::{load_all_sources, SourceType, SourcesList};
use raptor_core::state::{deb_architecture, detect_architecture, State};

use crate::term;

pub struct Context {
    pub state: State,
    pub index: PackageIndex,
    pub sources: SourcesList,
    pub arch: String,
    pub install_root: PathBuf,
    pub cache_dir: PathBuf,
    pub archives_dir: PathBuf,
    pub state_path: PathBuf,
}

impl Context {
    pub fn load() -> anyhow::Result<Self> {
        let config = RaptorConfig::load().unwrap_or_default();
        config.apply_env();

        let state_path = config.paths.state.clone();
        let install_root = config.paths.root.clone();
        let cache_dir = config.paths.cache.clone();
        let archives_dir = config.paths.archives.clone();

        let state = State::load(&state_path)?;
        let sources = match load_all_sources() {
            Ok(s) => s,
            Err(e) => {
                term::warn_line(format!("failed to load sources: {e}"));
                SourcesList::default()
            }
        };
        let arch = deb_architecture(&detect_architecture());

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
            if !is_remote(&source.uri) {
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

        Ok(Self {
            state,
            index,
            sources,
            arch,
            install_root,
            cache_dir,
            archives_dir,
            state_path,
        })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.state.save()?;
        Ok(())
    }
}

fn is_remote(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}
