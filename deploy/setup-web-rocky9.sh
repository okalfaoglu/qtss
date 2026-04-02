#!/usr/bin/env bash
# Install Node/npm (Rocky 9+), build QTSS web, optional first-time .env.
# Run as root or with sudo once:  sudo bash deploy/setup-web-rocky9.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="${REPO_ROOT}/web"

node_major() {
  node -v | sed 's/^v//' | cut -d. -f1
}

ensure_node_18() {
  local major
  major="$(node_major)"
  if [[ "${major}" -ge 18 ]]; then
    return 0
  fi
  return 1
}

install_nodejs_dnf_default() {
  echo "Installing Node.js and npm (dnf default package)..."
  dnf install -y nodejs npm
}

# Rocky / RHEL AppStream: switch from nodejs:16 (or default) to nodejs:20
upgrade_node_appstream_20() {
  echo "Node $(node -v) is below 18; enabling AppStream module nodejs:20..."
  dnf module reset -y nodejs 2>/dev/null || true
  dnf module enable -y nodejs:20
  dnf install -y nodejs npm
}

if ! command -v node >/dev/null 2>&1; then
  install_nodejs_dnf_default
fi

if ! ensure_node_18; then
  upgrade_node_appstream_20
fi

if ! ensure_node_18; then
  echo "Node 18+ still not satisfied; found $(node -v 2>/dev/null || echo none)." >&2
  echo "Try manually: dnf module list nodejs && dnf module install nodejs:20 -y" >&2
  echo "Or install Node 20+ from https://github.com/nodesource/distributions" >&2
  exit 1
fi

echo "Using $(node -v), npm $(npm -v)"

cd "${WEB_DIR}"
if [[ ! -f .env ]]; then
  if [[ -f .env.example ]]; then
    cp -a .env.example .env
    echo "Created web/.env from .env.example — set OAuth client secret and dev passwords."
  fi
fi

echo "npm ci..."
npm ci
echo "npm run build..."
npm run build
echo "Done. Manual preview: cd ${WEB_DIR} && npm run preview:bind"
echo "systemd: sudo cp ${REPO_ROOT}/deploy/systemd/qtss-web.service.example /etc/systemd/system/qtss-web.service && sudo systemctl daemon-reload && sudo systemctl enable --now qtss-web"
