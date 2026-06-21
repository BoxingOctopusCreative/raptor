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

            let entry = self
                .index
                .get_best(&name, &self.arch)
                .ok_or_else(|| Error::PackageNotFound(name.clone()))?;

            if self.state.is_installed(&name) {
                if let Some(installed) = self.state.get(&name) {
                    if deb_version_compare(&entry.control.version, &installed.version)
                        != std::cmp::Ordering::Greater
                    {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            self.check_conflicts(&entry.control)?;
            to_install.insert(name.clone());

            self.resolve_dependency_groups(
                &entry.control.predepends_groups(),
                &mut queue,
                &to_install,
            )?;
            self.resolve_dependency_groups(
                &entry.control.depends_groups(),
                &mut queue,
                &to_install,
            )?;

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

    pub fn plan_remove(&self, package_names: &[String], purge: bool) -> Result<InstallPlan> {
        let mut plan = InstallPlan::default();
        for name in package_names {
            let present = if purge {
                self.state.is_purgeable(name)
            } else {
                self.state.is_removable(name)
            };
            if !present {
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

    fn resolve_dependency_groups(
        &self,
        groups: &[Vec<Dependency>],
        queue: &mut VecDeque<String>,
        chosen: &HashSet<String>,
    ) -> Result<()> {
        for group in groups {
            self.resolve_dependency_group(group, queue, chosen)?;
        }
        Ok(())
    }

    fn resolve_dependency_group(
        &self,
        alternatives: &[Dependency],
        queue: &mut VecDeque<String>,
        chosen: &HashSet<String>,
    ) -> Result<()> {
        for dep in alternatives {
            if self.is_dependency_satisfied(dep, chosen) {
                return Ok(());
            }
        }

        for dep in alternatives {
            if self.try_queue_dependency(dep, queue, chosen)? {
                return Ok(());
            }
        }

        let names: Vec<_> = alternatives.iter().map(|dep| dep.name.as_str()).collect();
        Err(Error::DependencyConflict(format!(
            "unable to satisfy dependency: {}",
            names.join(" | ")
        )))
    }

    fn is_dependency_satisfied(&self, dep: &Dependency, chosen: &HashSet<String>) -> bool {
        if !self.applies_to_arch(dep) {
            return true;
        }

        if self.state.is_installed(&dep.name) {
            if let Some(installed) = self.state.get(&dep.name) {
                if dep.is_satisfied_by(&installed.name, &installed.version) {
                    return true;
                }
            }
        }

        for package in chosen {
            if self.package_satisfies_dep(package, dep) {
                return true;
            }
        }

        for name in self.state.installed_names() {
            if self.package_satisfies_dep(&name, dep) {
                return true;
            }
        }

        false
    }

    fn package_satisfies_dep(&self, package: &str, dep: &Dependency) -> bool {
        let Some(entry) = self.index.get_best(package, &self.arch) else {
            return package == dep.name && dep.constraints.is_empty();
        };

        if package == dep.name {
            return dep.constraints.is_empty()
                || dep
                    .constraints
                    .iter()
                    .all(|constraint| constraint.satisfies(&entry.control.version));
        }

        entry
            .control
            .provides_list()
            .iter()
            .any(|provided| provided == &dep.name)
    }

    fn try_queue_dependency(
        &self,
        dep: &Dependency,
        queue: &mut VecDeque<String>,
        chosen: &HashSet<String>,
    ) -> Result<bool> {
        if !self.applies_to_arch(dep) {
            return Ok(true);
        }

        if let Some(entry) = self.index.get_best(&dep.name, &self.arch) {
            if dep.constraints.is_empty()
                || dep
                    .constraints
                    .iter()
                    .all(|constraint| constraint.satisfies(&entry.control.version))
            {
                if !chosen.contains(&dep.name) && !self.state.is_installed(&dep.name) {
                    queue.push_back(dep.name.clone());
                }
                return Ok(true);
            }
        }

        if let Some(provider) = self
            .find_providers(&dep.name)
            .into_iter()
            .find(|provider| !chosen.contains(provider) && !self.state.is_installed(provider))
        {
            queue.push_back(provider);
            return Ok(true);
        }

        Ok(false)
    }

    fn applies_to_arch(&self, dep: &Dependency) -> bool {
        dep.arch_filter
            .as_ref()
            .is_none_or(|filter| filter == &self.arch)
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

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};
    use std::path::PathBuf;

    use super::*;
    use crate::acquire::{local_deb_index_entry, DIRECT_DEB_PRIORITY};
    use crate::control::ControlFile;
    use crate::dependency::parse_dependency_groups;
    use crate::repository::{PackageIndex, PackageIndexEntry};
    use crate::state::State;

    fn entry(
        package: &str,
        version: &str,
        arch: &str,
        depends: &str,
        provides: &str,
    ) -> PackageIndexEntry {
        PackageIndexEntry {
            control: ControlFile {
                package: package.into(),
                version: version.into(),
                architecture: arch.into(),
                depends: depends.into(),
                provides: provides.into(),
                ..ControlFile::default()
            },
            file_path: PathBuf::from(format!("/pool/{package}.deb")),
            source_uri: None,
            packages_index_path: None,
            signed_by: None,
            suite: None,
            component: None,
            repo_priority: 500,
        }
    }

    fn index_with(entries: Vec<PackageIndexEntry>) -> PackageIndex {
        let mut index = PackageIndex::default();
        for entry in entries {
            index
                .packages
                .entry(entry.control.package.clone())
                .or_default()
                .push(entry);
        }
        index
    }

    #[test]
    fn resolves_or_dependency_with_virtual_provider() {
        let index = index_with(vec![
            entry(
                "nginx-common",
                "1.0-1",
                "all",
                "debconf (>= 0.5) | debconf-2.0",
                "",
            ),
            entry("debconf", "1.5.86", "all", "", "debconf-2.0"),
            entry(
                "nginx",
                "1.0-1",
                "arm64",
                "nginx-common (= 1.0-1)",
                "",
            ),
        ]);
        let state = State::default();
        let resolver = Resolver::new(&index, &state, "arm64");
        let plan = resolver
            .plan_install(&["nginx".to_string()])
            .expect("plan nginx");

        let packages: Vec<_> = plan.actions.iter().map(|a| a.package.as_str()).collect();
        assert!(packages.contains(&"nginx"));
        assert!(packages.contains(&"nginx-common"));
        assert!(packages.contains(&"debconf"));
        assert_eq!(packages.len(), 3);
    }

    #[test]
    fn satisfies_virtual_dependency_from_queued_provider() {
        let index = index_with(vec![entry(
            "debconf",
            "1.5.86",
            "all",
            "",
            "debconf-2.0",
        )]);
        let state = State::default();
        let resolver = Resolver::new(&index, &state, "arm64");
        let mut queue = VecDeque::from(["debconf".to_string()]);
        let mut chosen = HashSet::new();
        chosen.insert("debconf".to_string());

        let group = parse_dependency_groups("debconf (>= 0.5) | debconf-2.0");
        resolver
            .resolve_dependency_group(&group[0], &mut queue, &chosen)
            .expect("virtual dep satisfied by queued debconf");
    }

    #[test]
    fn installs_direct_deb_when_runtime_arch_differs_from_compile_time() {
        let index = index_with(vec![local_deb_index_entry(
            PathBuf::from("/var/cache/apt/archives/raptor_0.6.0_amd64.deb"),
            ControlFile {
                package: "raptor".into(),
                version: "0.6.0".into(),
                architecture: "amd64".into(),
                ..Default::default()
            },
        )]);
        assert_eq!(
            index
                .packages
                .get("raptor")
                .and_then(|entries| entries.first())
                .map(|entry| entry.repo_priority),
            Some(DIRECT_DEB_PRIORITY)
        );

        let state = State::default();
        let resolver = Resolver::new(&index, &state, "arm64");
        let plan = resolver
            .plan_install(&["raptor".to_string()])
            .expect("direct deb should bypass arch filter");
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].package, "raptor");
        assert_eq!(plan.actions[0].version, "0.6.0");
    }
}
