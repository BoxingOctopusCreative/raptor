use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::repo_config::RepoConfig;

pub fn scaffold_private_repo(root: &Path, suite: &str, component: &str) -> Result<RepoConfig> {
    let config = RepoConfig::private(suite, component);
    write_repo_layout(root, &config)
}

pub fn scaffold_ppa_repo(root: &Path, owner: &str, name: &str, suite: &str) -> Result<RepoConfig> {
    let config = RepoConfig::ppa(owner, name, suite);
    write_repo_layout(root, &config)
}

fn write_repo_layout(root: &Path, config: &RepoConfig) -> Result<RepoConfig> {
    fs::create_dir_all(root.join("pool"))?;
    for component in &config.components {
        for arch in &config.architectures {
            fs::create_dir_all(root.join(format!(
                "dists/{}/{}/binary-{arch}",
                config.suite, component
            )))?;
        }
    }

    config.save(&root.join(RepoConfig::FILE_NAME))?;

    let keyring = config
        .signing
        .as_ref()
        .map(|s| s.keyring.as_str())
        .unwrap_or("keyrings/repo.gpg");
    let sources = match config.kind {
        crate::repo_config::RepoKind::Ppa => {
            let ppa = config.ppa.as_ref().unwrap();
            format!(
                "# PPA-style source for {owner}/{name}\n\
                 deb [signed-by={keyring}] {uri} {suite} {components}\n",
                owner = ppa.owner,
                name = ppa.name,
                uri = ppa.launchpad_uri.as_deref().unwrap_or("https://example.com/ubuntu"),
                suite = config.suite,
                components = config.components.join(" ")
            )
        }
        _ => format!(
            "# Private Raptor repository\n\
             deb [signed-by={keyring}] file:{root} {suite} {components}\n",
            root = root.display(),
            suite = config.suite,
            components = config.components.join(" ")
        ),
    };
    fs::write(root.join("sources.list.snippet"), sources)?;

    if let Some(signing) = &config.signing {
        fs::create_dir_all(root.join("keyrings"))?;
        let instructions = format!(
            "# Signing key setup\n\n\
             1. Generate a key:\n\
                gpg --full-generate-key\n\n\
             2. Export for apt:\n\
                gpg --armor --export YOUR_KEY_ID | gpg --dearmor -o {keyring}\n\n\
             3. Sign Release when publishing:\n\
                raptor repo index --repo {root}\n",
            keyring = signing.keyring,
            root = root.display()
        );
        fs::write(root.join("SIGNING.md"), instructions)?;
    }

    Ok(config.clone())
}
