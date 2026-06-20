use std::io::{self, Write};

use raptor_core::acquire::{build_package_url, ensure_deb, AcquireContext};
use raptor_core::deb::{extract_deb_to, read_deb};
use raptor_core::remote::fetch_remote_indexes;
use raptor_core::repository::{scan_pool_directory, write_packages_index};
use raptor_core::resolver::{ActionKind, Resolver};

use crate::context::Context;
use crate::global::GlobalOpts;

pub fn cmd_update() -> anyhow::Result<()> {
    let ctx = Context::load()?;
    println!("Hit:1 Raptor package index update");

    for (url, _) in fetch_remote_indexes(&ctx.sources, &ctx.cache_dir, &ctx.arch)? {
        println!("Get:1 {url} [Updated]");
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
        println!("Get:1 file:{} [Updated]", root.display());
    }

    println!("Reading package lists... Done");
    Ok(())
}

pub fn cmd_upgrade(global: &GlobalOpts) -> anyhow::Result<()> {
    let mut ctx = Context::load()?;
    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_upgrade()?;

    if plan.actions.is_empty() {
        println!("0 upgraded, 0 newly installed, 0 to remove.");
        return Ok(());
    }

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan)?;
    }
    Ok(())
}

pub fn cmd_install(packages: Vec<String>, global: &GlobalOpts) -> anyhow::Result<()> {
    if packages.is_empty() {
        anyhow::bail!("no packages specified");
    }

    let mut ctx = Context::load()?;
    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_install(&packages)?;

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan)?;
    }
    Ok(())
}

pub fn cmd_remove(packages: Vec<String>, purge: bool, global: &GlobalOpts) -> anyhow::Result<()> {
    if packages.is_empty() {
        anyhow::bail!("no packages specified");
    }

    let mut ctx = Context::load()?;
    let resolver = Resolver::new(&ctx.index, &ctx.state, &ctx.arch);
    let plan = resolver.plan_remove(&packages)?;

    print_plan(&plan.actions);
    if !global.dry_run && confirm(global.yes)? {
        execute_plan(&mut ctx, &plan)?;
        if purge {
            for pkg in &packages {
                println!("Purging configuration for {pkg} ...");
            }
        }
    }
    Ok(())
}

pub fn cmd_search(pattern: String) -> anyhow::Result<()> {
    let ctx = Context::load()?;
    let results = ctx.index.search(&pattern);
    for entry in results {
        let desc = entry.control.description.lines().next().unwrap_or("");
        println!("{} - {}", entry.control.package, desc);
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
    println!("Package: {}", c.package);
    println!("Version: {}", c.version);
    println!("Architecture: {}", c.architecture);
    if !c.maintainer.is_empty() {
        println!("Maintainer: {}", c.maintainer);
    }
    if !c.depends.is_empty() {
        println!("Depends: {}", c.depends);
    }
    println!("Description: {}", c.description);
    if !c.filename.is_empty() {
        println!("Filename: {}", c.filename);
    }
    if !c.size.is_empty() {
        println!("Size: {}", c.size);
    }
    Ok(())
}

pub fn cmd_list() -> anyhow::Result<()> {
    let ctx = Context::load()?;
    let mut names = ctx.state.installed_names();
    names.sort();
    for name in names {
        if let Some(pkg) = ctx.state.get(&name) {
            println!(
                "{}/{} {} {}",
                pkg.name, pkg.version, pkg.architecture, pkg.status
            );
        }
    }
    Ok(())
}

fn print_plan(actions: &[raptor_core::resolver::InstallAction]) {
    for action in actions {
        match action.action {
            ActionKind::Install => println!(
                "Inst {} {} [{}]",
                action.package, action.version, action.deb_path.display()
            ),
            ActionKind::Upgrade => println!(
                "Upgr {} {} [{}]",
                action.package, action.version, action.deb_path.display()
            ),
            ActionKind::Remove => println!("Remv {} {}", action.package, action.version),
        }
    }
}

fn confirm(yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }
    print!("Do you want to continue? [Y/n] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(!input.trim().eq_ignore_ascii_case("n"))
}

fn execute_plan(
    ctx: &mut Context,
    plan: &raptor_core::resolver::InstallPlan,
) -> anyhow::Result<()> {
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
                    println!(
                        "Get:1 {} [{}]",
                        build_package_url(
                            entry.source_uri.as_ref().unwrap(),
                            &entry.control.filename
                        ),
                        deb_path.display()
                    );
                }
                let deb = read_deb(&deb_path)?;
                extract_deb_to(&ctx.install_root, &deb_path)?;
                ctx.state.install(&deb.control);
                println!("Setting up {} ({}) ...", action.package, action.version);
            }
            ActionKind::Remove => {
                ctx.state.remove(&action.package);
                println!("Removing {} ({}) ...", action.package, action.version);
            }
        }
    }
    ctx.save()?;
    Ok(())
}
