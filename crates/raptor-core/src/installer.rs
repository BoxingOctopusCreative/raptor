use std::path::PathBuf;

use crate::acquire::{ensure_deb, AcquireContext};
use crate::deb::{extract_deb_to, read_deb};
use crate::error::Result;
use crate::repository::PackageIndex;
use crate::resolver::{ActionKind, InstallPlan};
use crate::state::State;

pub struct InstallContext {
    pub state: State,
    pub index: PackageIndex,
    pub arch: String,
    pub install_root: PathBuf,
    pub archives_dir: PathBuf,
}

impl InstallContext {
    pub fn apply_plan(&mut self, plan: &InstallPlan) -> Result<()> {
        for action in &plan.actions {
            match action.action {
                ActionKind::Install | ActionKind::Upgrade => {
                    let entry = self
                        .index
                        .get_best(&action.package, &self.arch)
                        .ok_or_else(|| {
                            crate::error::Error::PackageNotFound(format!(
                                "{} not found in index",
                                action.package
                            ))
                        })?;
                    let acquire_ctx = AcquireContext {
                        archives_dir: self.archives_dir.clone(),
                    };
                    let deb_path = ensure_deb(entry, &acquire_ctx)?;
                    let deb = read_deb(&deb_path)?;
                    extract_deb_to(&self.install_root, &deb_path)?;
                    self.state.install(&deb.control);
                }
                ActionKind::Remove => {
                    self.state.remove(&action.package);
                }
            }
        }
        self.state.save()
    }
}
