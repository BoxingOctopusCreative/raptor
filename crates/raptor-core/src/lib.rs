pub mod acquire;
pub mod config;
pub mod control;
pub mod deb;
pub mod dependency;
pub mod error;
pub mod installer;
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
pub mod state;
pub mod trust;
pub mod unattended;
pub mod verify;

pub use acquire::{build_package_url, ensure_deb, AcquireContext};
pub use config::{default_config_path, save_yaml_file, RaptorConfig, UnattendedConfig};
pub use control::ControlFile;
pub use deb::{read_deb, write_deb, DebArchive};
pub use dependency::{Dependency, VersionConstraint};
pub use error::{Error, Result};
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
pub use sources::{SourceEntry, SourcesList};
pub use state::{InstalledPackage, State};
pub use unattended::{daemon_loop, run_unattended_cycle, UnattendedReport};
