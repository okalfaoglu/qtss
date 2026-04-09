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
#   GIT_PULL_REBASE=1   — after fetch, `git rebase origin/<branch>` (local commits replay on top)
#   GIT_PULL_MERGE=1    — after fetch, `git merge --no-edit origin/<branch>` (merge commit if needed)
#   (default)           — `git merge --ff-only` only; fails if branches diverged — see deploy README note
#   SUDO                — default: sudo (use "" if root)
#   CARGO_PACKAGES      — default: "-p qtss-api -p qtss-worker"; empty string = full workspace
set -euo pipefail

ROOT="${QTSS_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)}"
cd "$ROOT"

SUDO="${SUDO:-sudo}"
REMOTE="${GIT_REMOTE:-origin}"
UNITS="${QTSS_SYSTEMD_UNITS:-qtss-api qtss-worker qtss-web qtss-web-v2}"
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
  BR=$(git rev-parse --abbrev-ref HEAD)
  UPSTREAM="${REMOTE}/${BR}"
  log "==> git: fetch ${REMOTE} (${BR})"
  git fetch "${REMOTE}"
  if [[ "${GIT_PULL_REBASE:-0}" == "1" ]]; then
    log "==> git: rebase onto ${UPSTREAM}"
    git rebase "${UPSTREAM}"
  elif [[ "${GIT_PULL_MERGE:-0}" == "1" ]]; then
    log "==> git: merge ${UPSTREAM}"
    git merge --no-edit "${UPSTREAM}"
  else
    if ! git merge --ff-only "${UPSTREAM}"; then
      log ""
      log "Git: fast-forward not possible (divergent branches). Typical on a server after a local commit that was never pushed."
      log "Pick one:"
      log "  1) Discard local commits and match GitHub:  git fetch ${REMOTE} && git reset --hard ${UPSTREAM} && chmod +x deploy/pull-build-restart.sh"
      log "  2) Replay local on top of remote:        GIT_PULL_REBASE=1 $0"
      log "  3) Merge remote into local:               GIT_PULL_MERGE=1 $0"
      log ""
      exit 1
    fi
  fi
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

build_web_dir() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    log "==> ${dir}/: directory missing, skip"
    return
  fi
  log "==> ${dir}: npm build"
  if [[ -f "${dir}/package-lock.json" ]]; then
    (cd "$dir" && npm ci && npm run build)
  else
    (cd "$dir" && npm install && npm run build)
  fi
}

if [[ "${SKIP_WEB:-0}" != "1" ]]; then
  build_web_dir web
  build_web_dir web-v2
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
