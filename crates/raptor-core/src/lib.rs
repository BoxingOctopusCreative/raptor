pub mod acquire;
pub mod config;
pub mod control;
pub mod deb;
pub mod dpkg_status;
pub mod dependency;
pub mod error;
pub mod fs_util;
pub mod installer;
pub mod keyring;
pub mod manifest;
pub mod mirror;
pub mod ppa;
pub mod release;
pub mod remote;
pub mod repo_config;
pub mod repository;
pub mod resolver;
pub mod scaffold;
pub mod sources;
pub mod sources_config;
pub mod state;
pub mod trust;
pub mod unattended;
pub mod verify;

pub use acquire::{
    acquire_direct_deb, build_package_url, enrich_direct_deb_control, ensure_deb, is_deb_spec,
    local_deb_index_entry, AcquireContext, DirectDeb, DIRECT_DEB_PRIORITY,
};
pub use config::{default_config_path, save_yaml_file, RaptorConfig, UnattendedConfig};
pub use control::ControlFile;
pub use deb::{
    apply_deferred_executables, read_deb, read_deb_control, write_deb, DebArchive,
    DeferredExecutable, ExtractDebResult,
};
pub use dependency::{Dependency, VersionConstraint};
pub use error::{Error, Result};
pub use keyring::ensure_dearmored_keyring;
pub use manifest::PackageManifest;
pub use mirror::{scaffold_mirror, sync_mirror, sync_mirror_indexes, sync_mirror_pool, MirrorConfig, MirrorSyncReport};
pub use mirror::mock::MockUpstream;
pub use ppa::{
    add_ppa, keyrings_dir_path, list_ppas, parse_ppa, remove_ppa, sources_list_d_path, PpaConfig,
    PpaRef,
};
pub use remote::fetch_remote_indexes;
pub use repo_config::{RepoConfig, RepoKind};
pub use repository::{PackageIndex, Repository};
pub use resolver::{InstallPlan, Resolver};
pub use scaffold::{scaffold_ppa_repo, scaffold_private_repo};
pub use sources::{load_all_sources, SourceEntry, SourcesList};
pub use sources_config::{
    convert_apt_sources, convert_apt_sources_from_config, default_sources_d_path,
    list_configured_repositories, load_sources_from_dir, reorder_repositories,
    repository_id, set_repository_priority, write_sources_to_dir, ConfiguredRepository,
    RepositoryEntry, SourcesYaml, DEFAULT_REPO_PRIORITY,
};
pub use state::{InstalledPackage, State};
pub use unattended::{daemon_loop, run_unattended_cycle, UnattendedReport};
