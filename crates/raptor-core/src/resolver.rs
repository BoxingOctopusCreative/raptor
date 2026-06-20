use std::collections::{HashSet, VecDeque};

use crate::control::ControlFile;
use crate::dependency::{deb_version_compare, Dependency};
use crate::error::{Error, Result};
use crate::repository::PackageIndex;
use crate::state::State;

#[derive(Debug, Clone)]
pub struct InstallAction {
    pub package: String,
    pub version: String,
    pub deb_path: std::path::PathBuf,
    pub action: ActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Install,
    Upgrade,
    Remove,
}

#[derive(Debug, Default)]
pub struct InstallPlan {
    pub actions: Vec<InstallAction>,
}

pub struct Resolver<'a> {
    index: &'a PackageIndex,
    state: &'a State,
    arch: String,
}

impl<'a> Resolver<'a> {
    pub fn new(index: &'a PackageIndex, state: &'a State, arch: &str) -> Self {
        Self {
            index,
            state,
            arch: arch.to_string(),
        }
    }

    pub fn plan_install(&self, package_names: &[String]) -> Result<InstallPlan> {
        let mut plan = InstallPlan::default();
        let mut to_install = HashSet::new();
        let mut queue: VecDeque<String> = package_names.iter().cloned().collect();

        while let Some(name) = queue.pop_front() {
            if to_install.contains(&name) {
                continue;
            }
            if self.state.is_installed(&name) {
                continue;
            }

            let entry = self
                .index
                .get_best(&name, &self.arch)
                .ok_or_else(|| Error::PackageNotFound(name.clone()))?;

            self.check_conflicts(&entry.control)?;
            to_install.insert(name.clone());

            for dep in entry.control.depends_list() {
                self.resolve_dependency(&dep, &mut queue, &to_install)?;
            }

            let action = if self.state.is_installed(&entry.control.package) {
                ActionKind::Upgrade
            } else {
                ActionKind::Install
            };

            plan.actions.push(InstallAction {
                package: entry.control.package.clone(),
                version: entry.control.version.clone(),
                deb_path: entry.file_path.clone(),
                action,
            });
        }

        plan.actions.sort_by(|a, b| a.package.cmp(&b.package));
        Ok(plan)
    }

    pub fn plan_remove(&self, package_names: &[String]) -> Result<InstallPlan> {
        let mut plan = InstallPlan::default();
        for name in package_names {
            if !self.state.is_installed(name) {
                return Err(Error::PackageNotFound(format!("{name} is not installed")));
            }
            if let Some(installed) = self.state.get(name) {
                plan.actions.push(InstallAction {
                    package: name.clone(),
                    version: installed.version.clone(),
                    deb_path: std::path::PathBuf::new(),
                    action: ActionKind::Remove,
                });
            }
        }
        Ok(plan)
    }

    pub fn plan_upgrade(&self) -> Result<InstallPlan> {
        let mut plan = InstallPlan::default();
        for name in self.state.installed_names() {
            let installed = self.state.get(&name).unwrap();
            if let Some(entry) = self.index.get_best(&name, &self.arch) {
                if deb_version_compare(&entry.control.version, &installed.version)
                    == std::cmp::Ordering::Greater
                {
                    plan.actions.push(InstallAction {
                        package: name.clone(),
                        version: entry.control.version.clone(),
                        deb_path: entry.file_path.clone(),
                        action: ActionKind::Upgrade,
                    });
                }
            }
        }
        Ok(plan)
    }

    fn resolve_dependency(
        &self,
        dep: &Dependency,
        queue: &mut VecDeque<String>,
        chosen: &HashSet<String>,
    ) -> Result<()> {
        if self.state.is_installed(&dep.name) {
            if let Some(installed) = self.state.get(&dep.name) {
                if dep.is_satisfied_by(&installed.name, &installed.version) {
                    return Ok(());
                }
            }
        }

        if chosen.contains(&dep.name) {
            return Ok(());
        }

        if let Some(entry) = self.index.get_best(&dep.name, &self.arch) {
            if dep.constraints.is_empty()
                || dep
                    .constraints
                    .iter()
                    .all(|c| c.satisfies(&entry.control.version))
            {
                queue.push_back(dep.name.clone());
                return Ok(());
            }
        }

        if let Some(provider) = self
            .find_providers(&dep.name)
            .into_iter()
            .find(|p| !chosen.contains(p) && !self.state.is_installed(p))
        {
            queue.push_back(provider);
            return Ok(());
        }

        Err(Error::DependencyConflict(format!(
            "unable to satisfy dependency: {}",
            dep.name
        )))
    }

    fn find_providers(&self, virtual_name: &str) -> Vec<String> {
        let mut providers = Vec::new();
        for (name, entries) in &self.index.packages {
            for entry in entries {
                if entry
                    .control
                    .provides_list()
                    .iter()
                    .any(|p| p == virtual_name)
                {
                    providers.push(name.clone());
                }
            }
        }
        providers
    }

    fn check_conflicts(&self, control: &ControlFile) -> Result<()> {
        for conflict in control.conflicts_list() {
            if let Some(installed) = self.state.get(&conflict.name) {
                if conflict.is_satisfied_by(&installed.name, &installed.version) {
                    return Err(Error::DependencyConflict(format!(
                        "{} conflicts with installed {}",
                        control.package, conflict.name
                    )));
                }
            }
        }
        Ok(())
    }
}
