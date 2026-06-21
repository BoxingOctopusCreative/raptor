# Raptor

Raptor is a Rust implementation of an APT-compatible Linux package manager. It reads standard Debian `.deb` packages, understands `sources.list`, resolves dependencies, and ships companion tools for building and publishing packages.

## Binaries

| Command                   | APT equivalent         | Purpose                                    |
|---------------------------|------------------------|--------------------------------------------|
| `raptor pkg get`          | `apt-get install`      | Install Package                            |
| `raptor pkg remove`       | `apt-get remove`       | Remove Package                             |
| `raptor pkg search`       | `apt-cache search`     | Search for Package in local Cache          |
| `raptor pkg info`         | `apt-cache show`       | Get info for package                       |
| `raptor upgrade`          | `apt-get dist-upgrade` | Upgrade installed packages                 |
| `raptor pkg list`         | `dpkg -l`              | List installed packages                    |
| `raptor pkg init`         | -                      | Initialize package manifest (raptor.yaml)  |
| `raptor pkg build`        | `dpkg-deb` / `debuild` | Build `.deb` package                       |
| `raptor pkg publish`      | `reprepro` / `aptly`   | Publish `.deb` package                     |
| `raptor repo update`      | `apt-get update`       | Update Package Cache                       |
| `raptor repo priority`    | `apt-cache policy`     | Get Priorities/Policies for Repos          |
| `raptor repo add`         | -                      | Add non-PPA repository                     |
| `raptor repo add-ppa`     | `add-apt-repository`   | Add PPA                                    |
| `raptor repo remove-ppa`  | -                      | Remove PPA                                 |
| `raptor repo list`        | -                      | List configured repositories               |
| `raptor repo create`      | -                      | Scaffold private/PPA repos and APT mirrors |
| `raptor repo index`       | -                      | Regenerate repository indexes              |
| `raptor repo sync`        | -                      | Sync an APT mirror from upstream           |
| `raptor repo apt-convert` | -                      | Convert APT sources.list to sources.d YAML |
| `raptor daemon`           | `unattended-upgrades`  | automatic update/upgrade daemon            |

## Quick start

### Install from a release

Tagged releases (`v*`, version must match `Cargo.toml`) are published to [GitHub Releases](https://github.com/BoxingOctopusCreative/raptor/releases) with:

| Asset | Description |
|-------|-------------|
| `raptor-{version}.tar.gz` / `.zip` | Source archives |
| `raptor-{version}-linux-amd64.tar.gz` | amd64 binary |
| `raptor-{version}-linux-arm64.tar.gz` | arm64 binary |
| `raptor_{version}_amd64.deb` | amd64 package |
| `raptor_{version}_arm64.deb` | arm64 package |

```bash
# .deb (recommended on Ubuntu/Debian)
sudo dpkg -i raptor_0.5.0_amd64.deb

# or standalone binary
tar -xzf raptor-0.5.0-linux-amd64.tar.gz
sudo cp raptor-0.5.0-linux-amd64/raptor /usr/local/bin/
```

### Build from source

```bash
cargo build --release
cargo install --path crates/raptor   # optional
sudo cp target/release/raptor /usr/local/bin/
```

Build a `.deb` of Raptor itself (uses `raptor pkg build` under the hood):

```bash
cargo build --release
./scripts/build-raptor-deb.sh   # -> packaging/target/raptor_{version}_{arch}.deb
```

### Cut a release

```bash
# 1. Bump [workspace.package] version in Cargo.toml
# 2. Commit, then tag (tag must match, e.g. v0.5.0 for version 0.5.0)
git tag v0.5.0
git push origin v0.5.0
```

CI runs tests, cross-compiles amd64/arm64 binaries, builds `.deb` packages, and publishes the GitHub release.

Global flags (apply to any subcommand):

```bash
raptor -y repo update          # assume yes
raptor --dry-run upgrade       # report only
raptor --config ./config.yaml pkg list
```

## Configuration (YAML)

Raptor-owned configuration uses YAML. When `/etc/raptor/sources.d/` contains repository files, Raptor loads them; otherwise it reads standard APT `sources.list` files.

Each file in `sources.d` defines one repository (or a multi-repo YAML document). This mirrors APT's `sources.list.d/*.list` layout.

Convert existing APT sources:

```bash
sudo raptor repo apt-convert
raptor repo apt-convert --stdout   # preview without writing
```

| File                         | Purpose                                                   |
|------------------------------|-----------------------------------------------------------|
| `/etc/raptor/config.yaml`    | Runtime paths, suite, security flags, unattended upgrades |
| `/etc/raptor/sources.d/`     | Per-repository YAML files (Raptor-native sources)         |
| `/var/lib/dpkg/status`       | Installed package database (dpkg format)                  |
| `raptor.yaml`                | Package build manifest (`raptor pkg init`)                |
| `repo.yaml`                  | Repository metadata (private, PPA, or mirror)             |
| `mirror.yaml`                | APT mirror upstream and sync settings                     |

Initialize system config:

```bash
sudo raptor config init --dir /etc/raptor
raptor config show
```

Example `config.yaml` lives in `examples/config/config.yaml`.

Legacy `RAPTOR_*` environment variables still override YAML when set.

## Environment variables

| Variable                | Default                   | Description |
|-------------------------|---------------------------|-------------|
| `RAPTOR_CONFIG`         | `/etc/raptor/config.yaml` | path to config.yaml |
| `RAPTOR_ROOT`           | `/`                       | filesystem root for package extraction |
| `RAPTOR_STATE`          | `/var/lib/dpkg/status`    | installed package database (dpkg format; YAML/JSON for dev) |
| `RAPTOR_CACHE`          | `/var/cache/raptor`       | package index cache |
| `RAPTOR_ARCHIVES`       | `/var/cache/apt/archives` | downloaded `.deb` cache |
| `RAPTOR_SOURCES_D`      | `/etc/raptor/sources.d`   | Raptor-native repository config directory |
| `RAPTOR_SOURCES`        | `/etc/apt/sources.list`   | override sources list path |
| `RAPTOR_SOURCES_LIST_D` | `/etc/apt/sources.list.d` | override sources.list.d directory |
| `RAPTOR_KEYRINGS_DIR`   | `/etc/apt/keyrings`       | override PPA keyring directory |
| `RAPTOR_SUITE`          | from `/etc/os-release`    | Ubuntu codename for PPAs (e.g. `jammy`) |
| `RAPTOR_ARCH`           | host architecture         | target architecture for package indexes |
| `RAPTOR_ALLOW_INSECURE` | unset                     | set to `1` to allow unsigned remote sources (not recommended) |
| `RAPTOR_DEBSIG_VERIFY`  | enabled                   | set to `0` to skip optional `debsig-verify` on install |

Use non-root paths for local testing:

```bash
export RAPTOR_ROOT="$HOME/.raptor/root"
export RAPTOR_STATE="$HOME/.raptor/state.yaml"
export RAPTOR_CACHE="$HOME/.raptor/cache"
mkdir -p "$RAPTOR_ROOT" "$(dirname "$RAPTOR_STATE")" "$RAPTOR_CACHE"
```

## Package manager usage

```bash
# Update indexes from configured sources
raptor repo update

# Search and inspect
raptor pkg search hello
raptor pkg info hello
raptor repo priority hello

# Install with dependency resolution
raptor -y pkg get hello

# Dist-upgrade / remove
raptor -y upgrade
raptor -y pkg remove hello
raptor -y pkg remove --purge hello   # remove configuration files too
raptor pkg list

# Repository pin priorities
raptor repo priority                  # list repository order
raptor repo priority --set repo-id --priority 900
raptor repo priority --reorder repo-a repo-b repo-c
raptor repo priority hello            # show version policy for a package
```

`sources.list` and `sources.list.d/*.list` are read from `/etc/apt/`. Local `file:` repositories and remote `http(s):` sources (including PPAs) are supported.

## PPAs (Personal Package Archives)

```bash
sudo raptor repo add-ppa ppa:git-core/cargo
raptor repo list
sudo raptor repo remove-ppa ppa:git-core/cargo
```

What happens when you add a PPA:

1. Detects your Ubuntu suite/codename from `/etc/os-release` (or `RAPTOR_SUITE`)
2. Fetches the PPA signing key from Launchpad and the Ubuntu keyserver
3. Writes `/etc/apt/keyrings/<owner>-ubuntu-<name>.gpg` (uses `gpg --dearmor` when available)
4. Writes `/etc/apt/sources.list.d/<owner>-ubuntu-<name>-<suite>.list`

For local testing without touching system paths:

```bash
export RAPTOR_SUITE=jammy
export RAPTOR_SOURCES_LIST_D="$HOME/.raptor/sources.list.d"
export RAPTOR_KEYRINGS_DIR="$HOME/.raptor/keyrings"
mkdir -p "$RAPTOR_SOURCES_LIST_D" "$RAPTOR_KEYRINGS_DIR"

raptor repo add-ppa ppa:git-core/cargo --skip-key   # skip key import if offline
raptor repo list
```

`raptor repo update` downloads and **cryptographically verifies** repository metadata before trusting package indexes:

1. Prefer `InRelease` (clearsigned), fall back to `Release` + `Release.gpg`
2. Verify signatures with the `signed-by` keyring via `gpgv` or `gpg`
3. Download `Packages` / `Packages.gz` only if listed in the signed `Release`
4. Verify `SHA256` (or `MD5`) checksums before caching indexes

Unsigned remote sources are rejected unless you explicitly set `RAPTOR_ALLOW_INSECURE=1` (not recommended).

On `raptor pkg get`, remote packages from signed sources require a GPG-verified update first:

1. Each cached `Packages` index gets a `Packages.raptor-trust` sidecar recording the keyring and checksum from the last verified update
2. Install refuses remote packages if the trust record is missing or the index was tampered with
3. Downloaded `.deb` files are checksum-verified against the trusted index
4. Optional detached `pool/.../package.deb.gpg` signatures are verified when present
5. Optional `debsig-verify` runs for embedded package signatures (disable with `RAPTOR_DEBSIG_VERIFY=0`)

## Building packages

Initialize a new package:

```bash
raptor pkg init myapp 1.0.0 amd64
```

Edit `raptor.yaml`, place files under `data/`, then build:

```bash
raptor pkg build
# -> target/myapp_1.0.0_amd64.deb
```

Or build from a Debian-style tree:

```bash
mkdir -p pkg/DEBIAN pkg/usr/local/bin
echo -e "Package: hello\nVersion: 1.0\nArchitecture: all\nMaintainer: you <you@example.com>\nDescription: demo" > pkg/DEBIAN/control
echo '#!/bin/sh\necho hello' > pkg/usr/local/bin/hello
chmod +x pkg/usr/local/bin/hello
raptor pkg build --root pkg --output hello_1.0_all.deb
```

## Publishing packages

Create a repository and publish:

```bash
raptor repo create --kind private --root ./repo --suite stable --component main
raptor pkg publish hello_1.0_all.deb --repo ./repo --suite stable --arch all
```

Reindex without adding a package:

```bash
raptor repo index --repo ./repo --suite stable --arch all
```

Point APT/Raptor at the repo:

```
deb file:/absolute/path/to/repo stable main
```

## Repository scaffolding and mirrors

Scaffold a private repository:

```bash
raptor repo create --kind private --root ./my-repo --suite stable --component main
raptor pkg publish hello_1.0_all.deb --repo ./my-repo --suite stable
```

Scaffold a PPA-style layout:

```bash
raptor repo create --kind ppa --root ./ppa-repo --owner myteam --name tools --suite jammy
```

Initialize and sync an APT mirror (indexes from upstream):

```bash
raptor repo create --kind mirror --root /var/raptor/mirror \
  --upstream http://archive.ubuntu.com/ubuntu --suite jammy
raptor repo sync --root /var/raptor/mirror
```

`mirror sync` copies signed `Packages` indexes and optionally pool `.deb` files (controlled by `sync_pool` and `pool_package_limit` in `mirror.yaml`).

### Testing mirrors without network or large downloads

Integration tests use `MockUpstream` (`raptor_core::mirror::mock`) — a tiny local `file://` APT repo built from the demo `.deb` (~KB, not GB):

```bash
bash examples/demo.sh   # creates examples/demo/hello-raptor_0.1.0_all.deb fixture
cargo test -p raptor-core mirror
```

Each mock upstream copies the demo deb N times, writes a minimal `Release` + `Packages`, and exercises index + pool sync with `pool_package_limit`.

## Unattended upgrades

Enable unattended upgrades in `/etc/raptor/config.yaml` (`unattended.enabled: true`; see `examples/config/config.yaml`), then run:

```bash
sudo raptor daemon --once          # single update/upgrade cycle
sudo raptor daemon                   # daemon loop (interval from config)
raptor --dry-run daemon --once       # report only, no changes
```

Install as a systemd service using `examples/config/raptor-daemon.service`.

## Development

```bash
cargo test                    # unit and integration tests
cargo test -p raptor-core     # library tests only
bash examples/demo.sh         # local demo fixture (~KB)
```

End-to-end smoke tests on Ubuntu via [Multipass](https://multipass.run/) (install/remove, PPAs, Docker repo, dogfood `.deb` install):

```bash
multipass launch -n test-vm --cpus 2 --memory 2G --disk 10G
multipass mount . test-vm:/home/ubuntu/raptor
./scripts/vm-smoke.sh all     # or: ./scripts/vm-smoke.sh pkg_get
```

Recreate the VM baseline snapshot after apt-convert changes: `RAPTOR_RECREATE_SNAPSHOT=1 ./scripts/vm-smoke.sh all`

## Architecture

```
crates/
  raptor-core/    # library: config, resolver, GPG, mirrors, unattended
  raptor/         # unified CLI (pkg, repo, daemon, config)
scripts/
  build-raptor-deb.sh   # build raptor_{version}_{arch}.deb
  vm-smoke.sh           # Multipass E2E smoke tests
```

## Scope and limitations

Raptor implements the core APT workflow for local, remote, and PPA repositories:

- `.deb` read/write (gzip-compressed control/data tarballs)
- Debian version comparison and dependency parsing
- Greedy dependency resolution with Provides/Conflicts
- `Packages` / `Packages.gz` and `Release` index generation
- Launchpad PPA add/remove with signing key import
- HTTP/HTTPS `Packages` index fetching on `raptor repo update`
- Signed repository updates: GPG verification of `InRelease` / `Release` + checksum validation of `Packages` indexes
- Remote `.deb` acquisition on `raptor pkg get` with GPG trust chain + checksum verification
- Per-package GPG: detached `.deb.gpg` signatures and `debsig-verify` when available
- YAML configuration for runtime, builds, repos, mirrors, and unattended upgrades
- `raptor daemon` for automatic update/upgrade cycles
- `raptor repo create` / `raptor repo sync` for scaffolding private repos, PPAs, and APT mirror layouts

Remote sources **must** use `signed-by=` unless `RAPTOR_ALLOW_INSECURE=1` is set. PPAs written by Raptor always include a keyring.

Downloaded packages are cached under `RAPTOR_ARCHIVES` (default `/var/cache/apt/archives`).

Not yet implemented (contributions welcome):

- Full pool mirroring at scale (parallel downloads, resume, by-component filtering)
- dpkg triggers, conffile prompts, and full dpkg status integration
- Multi-arch pinning, complex alternative groups, and full Aptitude-grade solver

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.