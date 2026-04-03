#!/usr/bin/env bash
# Pull latest from Git (default: origin + current branch), build Rust release binaries
# (qtss-api, qtss-worker) and web (npm), then restart systemd units (default: qtss-api,
# qtss-worker, qtss-web). WSL/systemd yoksa yalnızca derleme yapılır; servis adımları atlanır.
#
# Environment (optional):
#   QTSS_REPO_ROOT      — repo root (default: parent of deploy/)
#   QTSS_SYSTEMD_UNITS  — space-separated unit basenames, e.g. "qtss-api qtss-worker"
#   SKIP_GIT=1          — skip git pull
#   SKIP_RUST=1         — skip cargo build
#   SKIP_WEB=1          — skip npm build
#   SKIP_RESTART=1      — skip systemctl restart
#   GIT_REMOTE          — default: origin
#   GIT_PULL_ARGS       — extra args for git pull (default: --ff-only)
#   SUDO                — default: sudo (use "" if root)
#   CARGO_PACKAGES      — default: "-p qtss-api -p qtss-worker"; empty string = full workspace
set -euo pipefail

ROOT="${QTSS_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)}"
cd "$ROOT"

SUDO="${SUDO:-sudo}"
REMOTE="${GIT_REMOTE:-origin}"
PULL_ARGS=(--ff-only)
if [[ -n "${GIT_PULL_ARGS:-}" ]]; then
  # shellcheck disable=SC2206
  PULL_ARGS=(${GIT_PULL_ARGS})
fi
UNITS="${QTSS_SYSTEMD_UNITS:-qtss-api qtss-worker qtss-web}"
# unset → api+worker; CARGO_PACKAGES="" → entire workspace; else use verbatim
if [[ -z "${CARGO_PACKAGES+x}" ]]; then
  CARGO_PKGS="-p qtss-api -p qtss-worker"
elif [[ "${CARGO_PACKAGES}" == "" ]]; then
  CARGO_PKGS=""
else
  CARGO_PKGS="${CARGO_PACKAGES}"
fi

log() { printf '%s\n' "$*"; }

if [[ "${SKIP_GIT:-0}" != "1" ]]; then
  log "==> git: pull ${REMOTE} ($(git rev-parse --abbrev-ref HEAD))"
  git pull "${REMOTE}" "${PULL_ARGS[@]}"
else
  log "==> git: skipped (SKIP_GIT=1)"
fi

if [[ "${SKIP_RUST:-0}" != "1" ]]; then
  log "==> cargo: release build${CARGO_PKGS:+ ${CARGO_PKGS}}"
  if [[ -n "${CARGO_PKGS}" ]]; then
    # shellcheck disable=SC2086
    cargo build --release ${CARGO_PKGS}
  else
    cargo build --release
  fi
else
  log "==> cargo: skipped (SKIP_RUST=1)"
fi

if [[ "${SKIP_WEB:-0}" != "1" ]]; then
  if [[ ! -d web ]]; then
    log "==> web/: directory missing, skip"
  else
    log "==> web: npm build"
    if [[ -f web/package-lock.json ]]; then
      (cd web && npm ci && npm run build)
    else
      (cd web && npm install && npm run build)
    fi
  fi
else
  log "==> web: skipped (SKIP_WEB=1)"
fi

if [[ "${SKIP_RESTART:-0}" == "1" ]]; then
  log "==> systemd: skipped (SKIP_RESTART=1)"
  exit 0
fi

if ! command -v systemctl >/dev/null 2>&1; then
  log "==> systemd: systemctl not found — restart services manually."
  exit 0
fi

if ! systemctl is-system-running >/dev/null 2>&1; then
  log "==> systemd: not running — restart services manually (e.g. target/release/qtss-api)."
  exit 0
fi

log "==> systemd: restart ${UNITS}"
for u in ${UNITS}; do
  if systemctl cat "${u}.service" &>/dev/null; then
    ${SUDO} systemctl restart "${u}.service"
    log "    restarted ${u}.service"
  else
    log "    skip ${u}.service (unit file not found)"
  fi
done

log "==> done"
