use std::path::Path;

use raptor_core::config::RaptorConfig;

pub fn cmd_config_init(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let config_path = dir.join("config.yaml");
    if !config_path.exists() {
        RaptorConfig::write_init_template(&config_path)?;
        println!("Wrote {}", config_path.display());
    } else {
        println!("Already exists: {}", config_path.display());
    }
    Ok(())
}

pub fn cmd_config_show() -> anyhow::Result<()> {
    let config = RaptorConfig::load().unwrap_or_default();
    println!("{}", serde_yaml::to_string(&config)?);
    Ok(())
}
