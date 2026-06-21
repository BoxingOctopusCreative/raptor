pub mod commands;
pub mod context;
pub mod global;
pub mod term;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use commands::RepoCreateKind;
use commands::{
    cmd_config_init, cmd_config_show, cmd_daemon, cmd_pkg_build, cmd_pkg_get, cmd_pkg_info,
    cmd_pkg_init, cmd_pkg_list, cmd_pkg_publish, cmd_pkg_remove, cmd_pkg_search, cmd_repo_add,
    cmd_repo_add_ppa, cmd_repo_apt_convert, cmd_repo_create, cmd_repo_index, cmd_repo_list, cmd_repo_priority,
    cmd_repo_remove_ppa, cmd_repo_sync, cmd_repo_update, cmd_upgrade,
};
use global::GlobalOpts;

#[derive(Parser)]
#[command(name = "raptor", about = "APT-compatible package manager", version, color = clap::ColorChoice::Auto)]
pub struct Cli {
    /// Assume yes to all prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
    /// Report actions without applying changes
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// Path to config.yaml (default: /etc/raptor/config.yaml)
    #[arg(long, global = true, env = "RAPTOR_CONFIG")]
    pub config: Option<PathBuf>,
    /// Colorize output (auto detects terminal; respects NO_COLOR)
    #[arg(long, global = true, value_enum, default_value_t = clap::ColorChoice::Auto)]
    pub color: clap::ColorChoice,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Dist-upgrade installed packages
    Upgrade,
    /// Package operations
    Pkg(PkgArgs),
    /// Repository operations
    Repo(RepoArgs),
    /// Unattended upgrade daemon
    Daemon {
        /// Run a single cycle and exit
        #[arg(long)]
        once: bool,
    },
    /// Initialize or show Raptor YAML configuration
    Config(ConfigArgs),
}

#[derive(Args)]
pub struct PkgArgs {
    #[command(subcommand)]
    pub command: PkgCommands,
}

#[derive(Subcommand)]
pub enum PkgCommands {
    /// Install packages
    Get {
        /// Package names to install
        packages: Vec<String>,
    },
    /// Remove packages
    Remove {
        /// Package names to remove
        packages: Vec<String>,
        /// Remove configuration files as well
        #[arg(long)]
        purge: bool,
    },
    /// Search the package cache
    Search {
        /// Search pattern
        pattern: String,
    },
    /// Show package details
    Info {
        /// Package name
        package: String,
    },
    /// List installed packages
    List,
    /// Initialize a new package manifest (raptor.yaml)
    Init {
        /// Package name
        name: String,
        #[arg(short, long, default_value = "1.0.0")]
        version: String,
        #[arg(short, long, default_value = "all")]
        arch: String,
    },
    /// Build a .deb package
    Build {
        /// Path to raptor.yaml manifest
        #[arg(short, long, default_value = "raptor.yaml")]
        manifest: PathBuf,
        /// Build from a Debian-style tree (DEBIAN/control + files) instead of a manifest
        #[arg(short, long)]
        root: Option<PathBuf>,
        /// Output .deb path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Publish a .deb to a repository
    Publish {
        /// Path to .deb file
        deb: PathBuf,
        /// Repository root directory
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "stable")]
        suite: String,
        #[arg(long, default_value = "main")]
        component: String,
        #[arg(long, default_value = "amd64")]
        arch: String,
    },
}

#[derive(Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub command: RepoCommands,
}

#[derive(Subcommand)]
pub enum RepoCommands {
    /// Update package indexes from configured sources
    Update,
    /// Show or configure repository pin priorities
    Priority {
        /// Package names to show version policy for (omit to list repository order)
        packages: Vec<String>,
        /// Repository id to set pin priority for (use with --priority)
        #[arg(long)]
        set: Option<String>,
        /// Pin priority value (use with --set)
        #[arg(long)]
        priority: Option<i32>,
        /// Reorder repositories (first id = highest priority)
        #[arg(long, num_args = 1..)]
        reorder: Vec<String>,
    },
    /// Add a non-PPA repository
    Add {
        /// Repository URI
        uri: String,
        #[arg(long)]
        suite: String,
        #[arg(long, default_value = "main")]
        component: String,
        /// Path to signing keyring for signed-by=
        #[arg(long)]
        signed_by: Option<PathBuf>,
    },
    /// Add a Launchpad PPA
    AddPpa {
        /// PPA identifier (ppa:owner/repository or owner/repository)
        ppa: String,
        #[arg(short = 'u', long)]
        suite: Option<String>,
        #[arg(long)]
        skip_key: bool,
    },
    /// Remove a configured PPA
    RemovePpa {
        ppa: String,
        #[arg(short = 'u', long)]
        suite: Option<String>,
    },
    /// List configured repositories
    List,
    /// Scaffold a private repo, PPA layout, or APT mirror
    Create {
        #[arg(long, value_enum)]
        kind: RepoCreateKind,
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "stable")]
        suite: String,
        #[arg(long, default_value = "main")]
        component: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        upstream: Option<String>,
    },
    /// Regenerate Packages and Release indexes for a repository
    Index {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "stable")]
        suite: String,
        #[arg(long, default_value = "stable")]
        codename: String,
        #[arg(long, default_value = "main")]
        component: String,
        #[arg(long, default_value = "amd64")]
        arch: String,
    },
    /// Sync package indexes from an upstream mirror
    Sync {
        #[arg(long)]
        root: PathBuf,
    },
    /// Convert APT sources.list files to Raptor sources.d YAML files
    AptConvert {
        /// Output directory for per-repo YAML files (default: /etc/raptor/sources.d)
        #[arg(long, default_value = "/etc/raptor/sources.d")]
        output: PathBuf,
        /// APT sources.list path (default: from config)
        #[arg(long)]
        sources: Option<PathBuf>,
        /// APT sources.list.d directory (default: from config)
        #[arg(long)]
        sources_list_d: Option<PathBuf>,
        /// Print YAML to stdout instead of writing a file
        #[arg(long)]
        stdout: bool,
    },
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Write default /etc/raptor/config.yaml template
    Init {
        #[arg(long, default_value = "/etc/raptor")]
        dir: PathBuf,
    },
    /// Print the effective configuration
    Show,
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    term::init(cli.color);
    let global = GlobalOpts {
        yes: cli.yes,
        dry_run: cli.dry_run,
        config: cli.config,
    };
    global.apply();

    match cli.command {
        Commands::Upgrade => cmd_upgrade(&global),
        Commands::Pkg(args) => match args.command {
            PkgCommands::Get { packages } => cmd_pkg_get(packages, &global),
            PkgCommands::Remove { packages, purge } => {
                cmd_pkg_remove(packages, purge, &global)
            }
            PkgCommands::Search { pattern } => cmd_pkg_search(pattern),
            PkgCommands::Info { package } => cmd_pkg_info(package),
            PkgCommands::List => cmd_pkg_list(),
            PkgCommands::Init {
                name,
                version,
                arch,
            } => cmd_pkg_init(&name, &version, &arch),
            PkgCommands::Build {
                manifest,
                root,
                output,
            } => cmd_pkg_build(manifest, root, output),
            PkgCommands::Publish {
                deb,
                repo,
                suite,
                component,
                arch,
            } => cmd_pkg_publish(deb, repo, suite, component, arch),
        },
        Commands::Repo(args) => match args.command {
            RepoCommands::Update => cmd_repo_update(),
            RepoCommands::Priority {
                packages,
                set,
                priority,
                reorder,
            } => cmd_repo_priority(packages, set, priority, reorder),
            RepoCommands::Add {
                uri,
                suite,
                component,
                signed_by,
            } => cmd_repo_add(uri, suite, component, signed_by),
            RepoCommands::AddPpa {
                ppa,
                suite,
                skip_key,
            } => cmd_repo_add_ppa(ppa, suite, skip_key),
            RepoCommands::RemovePpa { ppa, suite } => cmd_repo_remove_ppa(ppa, suite),
            RepoCommands::List => cmd_repo_list(),
            RepoCommands::Create {
                kind,
                root,
                suite,
                component,
                owner,
                name,
                upstream,
            } => cmd_repo_create(kind, root, suite, component, owner, name, upstream),
            RepoCommands::Index {
                repo,
                suite,
                codename,
                component,
                arch,
            } => cmd_repo_index(repo, suite, codename, component, arch),
            RepoCommands::Sync { root } => cmd_repo_sync(root),
            RepoCommands::AptConvert {
                output,
                sources,
                sources_list_d,
                stdout,
            } => cmd_repo_apt_convert(output, sources, sources_list_d, stdout),
        },
        Commands::Daemon { once } => cmd_daemon(once, &global),
        Commands::Config(args) => match args.command {
            ConfigCommands::Init { dir } => cmd_config_init(&dir),
            ConfigCommands::Show => cmd_config_show(),
        },
    }
}
