#!/usr/bin/env bash
# Install Node/npm (Rocky 9+), build QTSS web, optional first-time .env.
# Run as root or with sudo once:  sudo bash deploy/setup-web-rocky9.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="${REPO_ROOT}/web"

if ! command -v node >/dev/null 2>&1; then
  echo "Installing Node.js and npm (dnf)..."
  dnf install -y nodejs npm
fi

node_ver="$(node -v | sed 's/^v//')"
# package.json engines.node >=18.18.0
major="${node_ver%%.*}"
if [[ "${major}" -lt 18 ]]; then
  echo "Node 18+ required; found $(node -v). Try: dnf module install nodejs:20 -y" >&2
  exit 1
fi

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
