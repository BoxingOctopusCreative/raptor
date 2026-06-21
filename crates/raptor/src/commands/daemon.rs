use raptor_core::config::RaptorConfig;
use raptor_core::unattended::{daemon_loop, run_unattended_cycle};

use crate::global::GlobalOpts;
use crate::term;

pub fn cmd_daemon(once: bool, global: &GlobalOpts) -> anyhow::Result<()> {
    let config = RaptorConfig::load().unwrap_or_default();
    let apply = !global.dry_run;

    if once {
        let report = run_unattended_cycle(&config, apply)?;
        if report.updated {
            term::success_line("Indexes updated");
        }
        if report.upgraded.is_empty() {
            term::note_line("No packages to upgrade");
        } else {
            term::success_line(format!("Packages upgraded: {}", report.upgraded.join(", ")));
        }
        return Ok(());
    }

    if !config.unattended.enabled {
        term::warn_line("raptor daemon: unattended.enabled is false in config; exiting");
        return Ok(());
    }

    term::note_line(format!(
        "raptor daemon: starting (interval {}h, apply={})",
        config.unattended.interval_hours, apply
    ));
    daemon_loop(config, false, apply).map_err(|e| anyhow::anyhow!("{e}"))
}
