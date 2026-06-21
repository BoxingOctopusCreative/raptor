use raptor_core::acquire::{
    acquire_direct_deb, build_package_url, enrich_direct_deb_control, ensure_deb, is_deb_spec,
    local_deb_index_entry, AcquireContext,
};
use raptor_core::control::ControlFile;
use raptor_core::deb::{apply_deferred_executables, extract_deb_to, read_deb_control, remove_deb_from};
use raptor_core::remote::fetch_remote_indexes;
use raptor_core::repository::{scan_pool_directory, write_packages_index};
use raptor_core::resolver::{ActionKind, Resolver};

use crate::context::Context;
use crate::global::GlobalOpts;
use crate::term;

pub fn cmd_update() -> anyhow::Result<()> {
    let ctx = Context::load()?;
    term::hit("Raptor package index update");

    for (url, _) in fetch_remote_indexes(&ctx.sources, &ctx.cache_dir, &ctx.arch)? {
        term::get(format!("{url} [Updated]"));
    }

    for root in ctx.sources.local_repo_roots() {
        let index = scan_pool_directory(&root.join("pool"), &ctx.arch)?;
        for (component, _) in [("main", ""), ("contrib", ""), ("non-free", "")] {
            let packages_dir = root.join(format!(
                "dists/{}/{}/binary-{}/",
                ctx.sources
                    .entries
                    .first()
                    .map(|e| e.suite.as_str())
                    .unwrap_or("stable"),
                component,
                ctx.arch
            ));
            if packages_dir.exists() || !index.packages.is_empty() {
                std::fs::create_dir_all(&packages_dir)?;
                write_packages_index(&packages_dir.join("Packages"), &index)?;
                write_packages_index(&packages_dir.join("Packages.gz"), &index)?;
            }
        }
        term::get(format!("file:{} [Updated]", root.display()));
    }

    term::done("Reading package lists...");
    Ok(())
}

pub fn cmd_upgrade(global: &GlobalOpts) -> anyhow::Result<()> {
    let mut ctx = Context::load()?;
    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_upgrade()?;

    if plan.actions.is_empty() {
        term::note_line("0 upgraded, 0 newly installed, 0 to remove.");
        return Ok(());
    }

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan, false)?;
    }
    Ok(())
}

pub fn cmd_install(packages: Vec<String>, global: &GlobalOpts) -> anyhow::Result<()> {
    if packages.is_empty() {
        anyhow::bail!("no packages specified");
    }

    let mut ctx = Context::load()?;
    let (deb_specs, repo_packages): (Vec<_>, Vec<_>) =
        packages.into_iter().partition(|p| is_deb_spec(p));

    let acquire_ctx = AcquireContext {
        archives_dir: ctx.archives_dir.clone(),
    };
    let mut install_names = repo_packages;

    for spec in deb_specs {
        let direct = acquire_direct_deb(&spec, &acquire_ctx).map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(url) = direct.remote_spec {
            term::get(format!("{} [{}]", url, direct.path.display()));
        }
        let control = read_deb_control(&direct.path)?;
        let control = enrich_direct_deb_control(control, &spec);
        let package = control.package.clone();
        let entry = local_deb_index_entry(direct.path, control);
        ctx.index
            .packages
            .entry(package.clone())
            .or_default()
            .push(entry);
        install_names.push(package);
    }

    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_install(&install_names)?;

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan, false)?;
    }
    Ok(())
}

pub fn cmd_remove(packages: Vec<String>, purge: bool, global: &GlobalOpts) -> anyhow::Result<()> {
    if packages.is_empty() {
        anyhow::bail!("no packages specified");
    }

    let mut ctx = Context::load()?;
    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_remove(&packages, purge)?;

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan, purge)?;
    }
    Ok(())
}

pub fn cmd_search(pattern: String) -> anyhow::Result<()> {
    let ctx = Context::load()?;
    let results = ctx.index.search(&pattern);
    for entry in results {
        let desc = entry.control.description.lines().next().unwrap_or("");
        term::search_result(&entry.control.package, desc);
    }
    Ok(())
}

pub fn cmd_info(package: String) -> anyhow::Result<()> {
    let ctx = Context::load()?;
    let entry = ctx
        .index
        .get(&package)
        .ok_or_else(|| anyhow::anyhow!("package '{package}' not found"))?;

    let c = &entry.control;
    term::info_field("Package", &c.package);
    term::info_field("Version", &c.version);
    term::info_field("Architecture", &c.architecture);
    if !c.maintainer.is_empty() {
        term::info_field("Maintainer", &c.maintainer);
    }
    if !c.depends.is_empty() {
        term::info_field("Depends", &c.depends);
    }
    term::info_field("Description", &c.description);
    if !c.filename.is_empty() {
        term::info_field("Filename", &c.filename);
    }
    if !c.size.is_empty() {
        term::info_field("Size", &c.size);
    }
    Ok(())
}

pub fn cmd_list() -> anyhow::Result<()> {
    let ctx = Context::load()?;
    let mut names = ctx.state.installed_names();
    names.sort();
    for name in names {
        if let Some(pkg) = ctx.state.get(&name) {
            term::installed_pkg(&pkg.name, &pkg.version, &pkg.architecture, &pkg.status);
        }
    }
    Ok(())
}

fn print_plan(actions: &[raptor_core::resolver::InstallAction]) {
    for action in actions {
        match action.action {
            ActionKind::Install => term::plan_install(
                &action.package,
                &action.version,
                &action.deb_path.display().to_string(),
            ),
            ActionKind::Upgrade => term::plan_upgrade(
                &action.package,
                &action.version,
                &action.deb_path.display().to_string(),
            ),
            ActionKind::Remove => term::plan_remove(&action.package, &action.version),
        }
    }
}

fn confirm(yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }

    let input = term::confirm_read_line().map_err(|e| {
        anyhow::anyhow!("cannot read confirmation (try -y in non-interactive mode): {e}")
    })?;
    Ok(!input.trim().eq_ignore_ascii_case("n"))
}

fn execute_plan(
    ctx: &mut Context,
    plan: &raptor_core::resolver::InstallPlan,
    purge: bool,
) -> anyhow::Result<()> {
    let mut deferred = Vec::new();
    for action in &plan.actions {
        match action.action {
            ActionKind::Install | ActionKind::Upgrade => {
                let entry = ctx
                    .index
                    .get_best(&action.package, &ctx.arch)
                    .ok_or_else(|| {
                        anyhow::anyhow!("package {} not found in index", action.package)
                    })?;
                let acquire_ctx = AcquireContext {
                    archives_dir: ctx.archives_dir.clone(),
                };
                let deb_path = ensure_deb(entry, &acquire_ctx).map_err(|e| anyhow::anyhow!("{e}"))?;
                if !action.deb_path.exists() && entry.source_uri.is_some() {
                    term::get(format!(
                        "{} [{}]",
                        build_package_url(
                            entry.source_uri.as_ref().unwrap(),
                            &entry.control.filename
                        ),
                        deb_path.display()
                    ));
                }
                let control = read_deb_control(&deb_path)?;
                let extract = extract_deb_to(&ctx.install_root, &deb_path)?;
                deferred.extend(extract.deferred_executables);
                ctx.state.install(&control);
                term::setting_up(&action.package, &action.version);
            }
            ActionKind::Remove => {
                term::removing(&action.package, &action.version);
                if let Some(deb_path) = resolve_installed_deb(ctx, &action.package, &action.version)
                {
                    remove_deb_from(&ctx.install_root, &deb_path, purge)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                if purge {
                    ctx.state.purge(&action.package);
                } else if !ctx.state.remove(&action.package) {
                    anyhow::bail!("package {} is not installed", action.package);
                }
            }
        }
    }
    ctx.save()?;
    apply_deferred_executables(&deferred).map_err(|e| anyhow::anyhow!("{e}"))?;
    for item in &deferred {
        term::note_line(format!(
            "Scheduled replacement of {}; the upgrade will take effect after this command exits.",
            item.dest.display()
        ));
    }
    Ok(())
}

fn resolve_installed_deb(
    ctx: &Context,
    package: &str,
    version: &str,
) -> Option<std::path::PathBuf> {
    let installed = ctx.state.get(package)?;
    let control = ControlFile {
        package: package.to_string(),
        version: version.to_string(),
        architecture: installed.architecture.clone(),
        ..ControlFile::default()
    };

    let archives_path = ctx.archives_dir.join(control.full_name());
    if archives_path.is_file() {
        return Some(archives_path);
    }

    if let Some(entry) = ctx.index.get_best(package, &ctx.arch) {
        if entry.file_path.is_file() {
            return Some(entry.file_path.clone());
        }
        let indexed = ctx.archives_dir.join(entry.control.full_name());
        if indexed.is_file() {
            return Some(indexed);
        }
    }

    None
}
