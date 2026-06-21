#!/usr/bin/env bash
# Host-side E2E smoke tests for Raptor on Multipass.
# Usage: ./scripts/vm-smoke.sh [test-name|all]
#
# Each test: restore snapshot -> build -> prepare sources -> run command -> report.
# Recreate baseline after apt-convert changes: RAPTOR_RECREATE_SNAPSHOT=1 ./scripts/vm-smoke.sh all
set -euo pipefail

VM_NAME="${RAPTOR_VM_NAME:-test-vm}"
SNAPSHOT="${RAPTOR_SMOKE_SNAPSHOT:-smoke-base}"
REPO_MOUNT="/home/ubuntu/raptor"
RAPTOR="sudo ${REPO_MOUNT}/target/release/raptor --color never"
# Package available in resolute-security; used for install/remove/purge tests.
SMOKE_PKG="${RAPTOR_SMOKE_PKG:-nginx}"
DOCKER_URI="${RAPTOR_SMOKE_DOCKER_URI:-https://download.docker.com/linux/ubuntu}"
DOCKER_SUITE="${RAPTOR_SMOKE_DOCKER_SUITE:-resolute}"
DOCKER_KEY="${RAPTOR_SMOKE_DOCKER_KEY:-/etc/apt/keyrings/docker.asc}"
NEOVIM_PPA="${RAPTOR_SMOKE_PPA:-ppa:neovim-ppa/unstable}"

restore_vm() {
  echo "==> Restoring ${VM_NAME}.${SNAPSHOT}"
  multipass stop "$VM_NAME" >/dev/null 2>&1 || true
  multipass restore "${VM_NAME}.${SNAPSHOT}" --destructive
  multipass start "$VM_NAME" >/dev/null
}

build() {
  multipass exec "$VM_NAME" -- bash -lc "cd ${REPO_MOUNT} && cargo build --release"
}

# Re-convert APT sources so /etc/raptor/sources.d has every suite (not stale partial files).
prepare_sources() {
  multipass exec "$VM_NAME" -- bash -lc "
    set -euo pipefail
    sudo rm -f /etc/raptor/sources.d/*.yaml
    ${RAPTOR} repo apt-convert
    test \$(ls /etc/raptor/sources.d/*.yaml | wc -l) -ge 3
  "
}

run_test() {
  local name="$1"
  shift
  echo ""
  echo "========================================"
  echo "TEST: $name"
  echo "========================================"
  restore_vm
  build
  if ! multipass exec "$VM_NAME" -- bash -lc "set -euo pipefail; $*"; then
    echo "FAIL: $name" >&2
    return 1
  fi
  echo "PASS: $name"
}

run_repo_test() {
  local name="$1"
  shift
  echo ""
  echo "========================================"
  echo "TEST: $name"
  echo "========================================"
  restore_vm
  build
  prepare_sources
  if ! multipass exec "$VM_NAME" -- bash -lc "set -euo pipefail; $*"; then
    echo "FAIL: $name" >&2
    return 1
  fi
  echo "PASS: $name"
}

test_build() {
  run_test "build" "cd ${REPO_MOUNT} && ./target/release/raptor --version"
}

test_config_show() {
  run_test "config show" \
    "out=\$(${RAPTOR} config show 2>/dev/null) && echo \"\$out\" | head -5 | grep -q '^paths:'"
}

test_config_init() {
  run_test "config init" \
    "CFG=/tmp/raptor-smoke-config-init && \
     sudo rm -rf \$CFG && \
     ${RAPTOR} config init --dir \$CFG > /tmp/raptor-smoke-config-init.txt && \
     grep -q 'Wrote /tmp/raptor-smoke-config-init/config.yaml' /tmp/raptor-smoke-config-init.txt && \
     test -f \$CFG/config.yaml && \
     grep -q '^paths:' \$CFG/config.yaml && \
     grep -q 'sources_d: /etc/raptor/sources.d' \$CFG/config.yaml && \
     grep -q 'enabled: true' \$CFG/config.yaml && \
     ${RAPTOR} config init --dir \$CFG > /tmp/raptor-smoke-config-init-again.txt && \
     grep -q 'Already exists: /tmp/raptor-smoke-config-init/config.yaml' /tmp/raptor-smoke-config-init-again.txt && \
     ${RAPTOR} --config \$CFG/config.yaml config show > /tmp/raptor-smoke-config-show.txt && \
     grep -q '^paths:' /tmp/raptor-smoke-config-show.txt && \
     grep -q 'enabled: true' /tmp/raptor-smoke-config-show.txt"
}

test_daemon_once() {
  run_repo_test "daemon --once" \
    "CFG=/tmp/raptor-smoke-daemon && \
     sudo rm -rf \$CFG && \
     ${RAPTOR} config init --dir \$CFG && \
     ${RAPTOR} --config \$CFG/config.yaml --dry-run daemon --once > /tmp/raptor-smoke-daemon-once.txt 2>&1 && \
     grep -q 'Indexes updated' /tmp/raptor-smoke-daemon-once.txt"
}

test_daemon_disabled() {
  run_test "daemon (disabled)" \
    "CFG=/tmp/raptor-smoke-daemon-disabled && \
     sudo rm -rf \$CFG && \
     ${RAPTOR} config init --dir \$CFG && \
     sudo sed -i 's/enabled: true/enabled: false/' \$CFG/config.yaml && \
     ${RAPTOR} --config \$CFG/config.yaml daemon > /tmp/raptor-smoke-daemon-disabled.txt 2>&1 && \
     grep -q 'unattended.enabled is false' /tmp/raptor-smoke-daemon-disabled.txt"
}

test_repo_apt_convert() {
  run_test "repo apt-convert --stdout" \
    "${RAPTOR} repo apt-convert --stdout > /tmp/raptor-smoke-apt-convert-stdout.yaml && \
     grep -q '^repositories:' /tmp/raptor-smoke-apt-convert-stdout.yaml && \
     grep -q 'uri: http://archive.ubuntu.com/ubuntu' /tmp/raptor-smoke-apt-convert-stdout.yaml && \
     grep -q 'suite: resolute-security' /tmp/raptor-smoke-apt-convert-stdout.yaml"
}

test_repo_apt_convert_write() {
  run_test "repo apt-convert (write)" \
    "sudo rm -f /etc/raptor/sources.d/*.yaml && \
     ${RAPTOR} repo apt-convert > /tmp/raptor-smoke-apt-convert-write.txt && \
     grep -q 'Wrote 4 repository file(s) to /etc/raptor/sources.d' /tmp/raptor-smoke-apt-convert-write.txt && \
     test -f /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     test -f /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute-updates.yaml && \
     test -f /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute-backports.yaml && \
     test -f /etc/raptor/sources.d/security-ubuntu-com-ubuntu-resolute-security.yaml && \
     grep -q '^uri: http://archive.ubuntu.com/ubuntu' /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     grep -q '^suite: resolute$' /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     grep -q '^uri: http://security.ubuntu.com/ubuntu' /etc/raptor/sources.d/security-ubuntu-com-ubuntu-resolute-security.yaml && \
     grep -q '^suite: resolute-security' /etc/raptor/sources.d/security-ubuntu-com-ubuntu-resolute-security.yaml && \
     grep -q '^signed_by: /usr/share/keyrings/ubuntu-archive-keyring.gpg' /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     grep -q '^- main' /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     grep -q 'origin: /etc/apt/sources.list.d/ubuntu.sources' /etc/raptor/sources.d/archive-ubuntu-com-ubuntu-resolute.yaml && \
     ${RAPTOR} -y repo update > /tmp/raptor-smoke-apt-convert-update.txt 2>&1 && \
     grep -q 'Reading package lists' /tmp/raptor-smoke-apt-convert-update.txt && \
     ${RAPTOR} pkg search ${SMOKE_PKG} > /tmp/raptor-smoke-apt-convert-search.txt && \
     grep -q '^${SMOKE_PKG} ' /tmp/raptor-smoke-apt-convert-search.txt"
}

test_repo_update() {
  run_repo_test "repo update" \
    "${RAPTOR} -y repo update 2>&1 | grep -q 'Reading package lists'"
}

test_pkg_search() {
  run_repo_test "pkg search" \
    "${RAPTOR} -y repo update && ${RAPTOR} pkg search ${SMOKE_PKG} > /tmp/raptor-smoke-search.txt && grep -q '^${SMOKE_PKG} ' /tmp/raptor-smoke-search.txt"
}

test_pkg_info() {
  run_repo_test "pkg info" \
    "${RAPTOR} -y repo update && ${RAPTOR} pkg info ${SMOKE_PKG} > /tmp/raptor-smoke-info.txt && grep -q '^Package: ${SMOKE_PKG}' /tmp/raptor-smoke-info.txt"
}

test_repo_priority() {
  run_repo_test "repo priority" \
    "${RAPTOR} repo priority > /tmp/raptor-smoke-priority-list.txt && \
     grep -q 'Repository priority order' /tmp/raptor-smoke-priority-list.txt && \
     grep -q 'id: security-ubuntu-com-ubuntu-resolute-security' /tmp/raptor-smoke-priority-list.txt && \
     ${RAPTOR} repo priority --set security-ubuntu-com-ubuntu-resolute-security --priority 900 && \
     ${RAPTOR} repo priority > /tmp/raptor-smoke-priority-set.txt && \
     grep -q '\\[ 900\\].*security-ubuntu-com-ubuntu-resolute-security' /tmp/raptor-smoke-priority-set.txt && \
     ${RAPTOR} repo priority --reorder \
       security-ubuntu-com-ubuntu-resolute-security \
       archive-ubuntu-com-ubuntu-resolute-updates \
       archive-ubuntu-com-ubuntu-resolute \
       archive-ubuntu-com-ubuntu-resolute-backports && \
     ${RAPTOR} repo priority > /tmp/raptor-smoke-priority-reorder.txt && \
     grep -q '1\\. \\[1000\\].*security-ubuntu-com-ubuntu-resolute-security' /tmp/raptor-smoke-priority-reorder.txt && \
     ${RAPTOR} -y repo update && \
     ${RAPTOR} repo priority ${SMOKE_PKG} > /tmp/raptor-smoke-priority-pkg.txt && \
     grep -q '${SMOKE_PKG}:' /tmp/raptor-smoke-priority-pkg.txt && \
     grep -q 'priority' /tmp/raptor-smoke-priority-pkg.txt"
}

test_repo_add() {
  run_repo_test "repo add (docker)" \
    "sudo install -m 0755 -d /etc/apt/keyrings && \
     sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o ${DOCKER_KEY} && \
     sudo chmod a+r ${DOCKER_KEY} && \
     ${RAPTOR} repo add ${DOCKER_URI} --suite ${DOCKER_SUITE} --component stable --signed-by ${DOCKER_KEY} && \
     test -f /etc/apt/keyrings/docker.gpg && \
     ${RAPTOR} repo list > /tmp/raptor-smoke-repo-list.txt && \
     grep -q 'download.docker.com/linux/ubuntu' /tmp/raptor-smoke-repo-list.txt && \
     grep -q 'docker.gpg' /etc/apt/sources.list.d/raptor-download-docker-com-linux-ubuntu.list && \
     ${RAPTOR} -y repo update > /tmp/raptor-smoke-docker-update.txt 2>&1 && \
     grep -q 'download.docker.com/linux/ubuntu' /tmp/raptor-smoke-docker-update.txt && \
     test -d /var/cache/raptor/download.docker.com_linux_ubuntu/dists/${DOCKER_SUITE} && \
     ${RAPTOR} pkg search docker-ce > /tmp/raptor-smoke-docker-search.txt && \
     grep -q '^docker-ce ' /tmp/raptor-smoke-docker-search.txt && \
     ${RAPTOR} -y pkg get docker-ce-cli && \
     ${RAPTOR} pkg list > /tmp/raptor-smoke-docker-list.txt && \
     grep -q '^docker-ce-cli/' /tmp/raptor-smoke-docker-list.txt && \
     command -v docker >/dev/null && docker --version | grep -q Docker"
}

test_repo_add_ppa() {
  run_repo_test "repo add-ppa (neovim)" \
    "${RAPTOR} repo add-ppa ${NEOVIM_PPA} && \
     ${RAPTOR} repo list > /tmp/raptor-smoke-ppa-list.txt && \
     grep -q 'neovim-ppa/unstable' /tmp/raptor-smoke-ppa-list.txt && \
     ${RAPTOR} -y repo update > /tmp/raptor-smoke-ppa-update.txt 2>&1 && \
     grep -q 'ppa.launchpadcontent.net/neovim-ppa/unstable/ubuntu' /tmp/raptor-smoke-ppa-update.txt && \
     test -d /var/cache/raptor/ppa.launchpadcontent.net_neovim-ppa_unstable_ubuntu/dists/${DOCKER_SUITE} && \
     ${RAPTOR} pkg search neovim > /tmp/raptor-smoke-neovim-search.txt && \
     grep -q '^neovim ' /tmp/raptor-smoke-neovim-search.txt && \
     ${RAPTOR} -y pkg get neovim && \
     ${RAPTOR} pkg list > /tmp/raptor-smoke-neovim-list.txt && \
     grep -q '^neovim/' /tmp/raptor-smoke-neovim-list.txt && \
     command -v nvim >/dev/null && nvim --version | head -1 | grep -qi nvim"
}

test_repo_remove_ppa() {
  run_repo_test "repo remove-ppa (neovim)" \
    "${RAPTOR} repo add-ppa ${NEOVIM_PPA} && \
     ${RAPTOR} -y repo update > /tmp/raptor-smoke-ppa-add-update.txt 2>&1 && \
     grep -q 'ppa.launchpadcontent.net/neovim-ppa/unstable/ubuntu' /tmp/raptor-smoke-ppa-add-update.txt && \
     ${RAPTOR} repo list > /tmp/raptor-smoke-ppa-before-remove.txt && \
     grep -q 'neovim-ppa/unstable' /tmp/raptor-smoke-ppa-before-remove.txt && \
     test -f /etc/apt/sources.list.d/neovim-ppa-ubuntu-unstable-${DOCKER_SUITE}.list && \
     ${RAPTOR} repo remove-ppa ${NEOVIM_PPA} > /tmp/raptor-smoke-ppa-remove.txt && \
     grep -q 'PPA removed: ppa:neovim-ppa/unstable' /tmp/raptor-smoke-ppa-remove.txt && \
     test ! -f /etc/apt/sources.list.d/neovim-ppa-ubuntu-unstable-${DOCKER_SUITE}.list && \
     ! test -e /etc/apt/keyrings/neovim-ppa-ubuntu-unstable.gpg && \
     ! test -e /etc/apt/keyrings/neovim-ppa-ubuntu-unstable.asc && \
     ${RAPTOR} repo list > /tmp/raptor-smoke-ppa-after-remove.txt && \
     ! grep -q 'neovim-ppa/unstable' /tmp/raptor-smoke-ppa-after-remove.txt && \
     ${RAPTOR} -y repo update > /tmp/raptor-smoke-ppa-remove-update.txt 2>&1 && \
     ! grep -q 'ppa.launchpadcontent.net/neovim-ppa/unstable/ubuntu' /tmp/raptor-smoke-ppa-remove-update.txt && \
     ${RAPTOR} repo add-ppa ${NEOVIM_PPA} > /tmp/raptor-smoke-ppa-readd.txt && \
     grep -q 'PPA added: ppa:neovim-ppa/unstable' /tmp/raptor-smoke-ppa-readd.txt"
}

test_pkg_get() {
  run_repo_test "pkg get" \
    "${RAPTOR} -y repo update && ${RAPTOR} -y pkg get ${SMOKE_PKG} && ${RAPTOR} pkg list > /tmp/raptor-smoke-list.txt && grep -q '^${SMOKE_PKG}/' /tmp/raptor-smoke-list.txt"
}

test_pkg_list() {
  run_repo_test "pkg list" \
    "${RAPTOR} -y repo update && ${RAPTOR} -y pkg get ${SMOKE_PKG} && ${RAPTOR} pkg list > /tmp/raptor-smoke-list.txt && grep -q '^${SMOKE_PKG}/' /tmp/raptor-smoke-list.txt"
}

test_upgrade_dry_run() {
  run_repo_test "upgrade --dry-run" \
    "${RAPTOR} -y repo update && ${RAPTOR} --dry-run upgrade"
}

test_pkg_remove() {
  run_repo_test "pkg remove" \
    "${RAPTOR} -y repo update && ${RAPTOR} -y pkg get ${SMOKE_PKG} && ${RAPTOR} -y pkg remove ${SMOKE_PKG} && ${RAPTOR} pkg list > /tmp/raptor-smoke-list.txt && ! grep -q '^${SMOKE_PKG}/' /tmp/raptor-smoke-list.txt"
}

test_pkg_purge() {
  run_repo_test "pkg remove --purge" \
    "${RAPTOR} -y repo update && ${RAPTOR} -y pkg get ${SMOKE_PKG} && ${RAPTOR} -y pkg remove ${SMOKE_PKG} && ${RAPTOR} -y pkg remove --purge ${SMOKE_PKG} && ${RAPTOR} pkg list > /tmp/raptor-smoke-list.txt && ! grep -q '^${SMOKE_PKG}/' /tmp/raptor-smoke-list.txt"
}

test_pkg_init_build() {
  run_test "pkg init/build (local)" \
    "cd /tmp && rm -rf raptor-smoke-pkg && mkdir raptor-smoke-pkg && cd raptor-smoke-pkg && \
     ${REPO_MOUNT}/target/release/raptor pkg init smoke-test --version 0.1.0 --arch all && \
     mkdir -p data/usr/local/bin && echo '#!/bin/sh' > data/usr/local/bin/smoke-test && \
     echo 'echo smoke-ok' >> data/usr/local/bin/smoke-test && chmod +x data/usr/local/bin/smoke-test && \
     ${REPO_MOUNT}/target/release/raptor pkg build && test -f target/smoke-test_0.1.0_all.deb"
}

test_pkg_dogfood() {
  run_test "pkg dogfood (raptor deb)" \
    "ARCH=\$(dpkg --print-architecture) && \
     DOGFOOD=/tmp/raptor-dogfood && \
     rm -rf \$DOGFOOD && mkdir -p \$DOGFOOD/repo && \
     RAPTOR_BIN=${REPO_MOUNT}/target/release/raptor RAPTOR_CLI=${REPO_MOUNT}/target/release/raptor \
       ${REPO_MOUNT}/scripts/build-raptor-deb.sh > /tmp/raptor-smoke-dogfood-deb.txt && \
     DEB=\$(tail -1 /tmp/raptor-smoke-dogfood-deb.txt) && \
     test -f \"\$DEB\" && \
     ${RAPTOR} repo create --kind private --root \$DOGFOOD/repo --suite stable --component main && \
     ${RAPTOR} pkg publish \"\$DEB\" --repo \$DOGFOOD/repo --suite stable --component main --arch \$ARCH && \
     ${RAPTOR} repo add file:\$DOGFOOD/repo --suite stable --component main && \
     ${RAPTOR} repo list > /tmp/raptor-smoke-dogfood-repo-list.txt && \
     grep -q \"file:\$DOGFOOD/repo\" /tmp/raptor-smoke-dogfood-repo-list.txt && \
     ${RAPTOR} pkg search raptor > /tmp/raptor-smoke-dogfood-search.txt && \
     grep -q '^raptor ' /tmp/raptor-smoke-dogfood-search.txt && \
     ${RAPTOR} -y pkg get raptor && \
     test -x /usr/local/bin/raptor && \
     /usr/local/bin/raptor --version | grep -q '^raptor ' && \
     ${RAPTOR} pkg list > /tmp/raptor-smoke-dogfood-list.txt && \
     grep -q '^raptor/' /tmp/raptor-smoke-dogfood-list.txt"
}

TESTS=(
  test_build
  test_config_show
  test_config_init
  test_repo_apt_convert
  test_repo_apt_convert_write
  test_repo_update
  test_pkg_search
  test_pkg_info
  test_repo_priority
  test_repo_add
  test_repo_add_ppa
  test_repo_remove_ppa
  test_pkg_get
  test_pkg_list
  test_upgrade_dry_run
  test_pkg_remove
  test_pkg_purge
  test_pkg_init_build
  test_pkg_dogfood
  test_daemon_once
  test_daemon_disabled
)

main() {
  local target="${1:-all}"
  local failed=0

  if [[ "${RAPTOR_RECREATE_SNAPSHOT:-}" == "1" ]]; then
    echo "Recreating baseline snapshot ${SNAPSHOT}..." >&2
    multipass stop "$VM_NAME" >/dev/null 2>&1 || true
    multipass start "$VM_NAME" >/dev/null 2>&1 || multipass launch -n "$VM_NAME" --cpus 2 --memory 2G --disk 10G
    multipass exec "$VM_NAME" -- bash -lc "
      set -euo pipefail
      sudo rm -f /etc/raptor/sources.d/*.yaml
    " || true
    multipass snapshot "$VM_NAME" -n "$SNAPSHOT" -c "E2E smoke baseline"
    multipass start "$VM_NAME" >/dev/null
  elif ! multipass list --snapshots 2>/dev/null | grep -q "^${VM_NAME}[[:space:]].*${SNAPSHOT}"; then
    echo "Creating baseline snapshot ${SNAPSHOT}..." >&2
    multipass stop "$VM_NAME" >/dev/null 2>&1 || true
    multipass snapshot "$VM_NAME" -n "$SNAPSHOT" -c "E2E smoke baseline"
    multipass start "$VM_NAME" >/dev/null
  fi

  for fn in "${TESTS[@]}"; do
    if [[ "$target" != "all" && "$fn" != "test_${target}" && "$fn" != "$target" ]]; then
      continue
    fi
    if ! "$fn"; then
      failed=$((failed + 1))
    fi
  done

  if [[ "$failed" -gt 0 ]]; then
    echo ""
    echo "$failed test(s) failed"
    exit 1
  fi
  echo ""
  echo "All smoke tests passed."
}

main "${1:-all}"
