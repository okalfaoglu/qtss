#!/usr/bin/env bash
# Install + wire nginx as the QTSS reverse proxy on Rocky 9 / RHEL 9.
# Idempotent: re-running upgrades the config in place; safe to invoke
# from a redeploy script.
#
# Usage:
#   sudo bash deploy/install-nginx-rocky9.sh             # default :80, server_name _
#   sudo SERVER_NAME=qtss.example.com bash deploy/install-nginx-rocky9.sh
#
# What it does (and only what it does):
#   1. dnf install nginx (no-op if present)
#   2. Render deploy/nginx/qtss.conf.example into /etc/nginx/conf.d/qtss.conf,
#      substituting SERVER_NAME if provided.
#   3. Drop the default catch-all server (/etc/nginx/nginx.conf ships one
#      that listens on :80 and would shadow our conf.d entry).
#   4. nginx -t && systemctl enable --now nginx && systemctl reload nginx
#
# Reverts: remove /etc/nginx/conf.d/qtss.conf and `systemctl reload nginx`.
# v1 trafiği etkilenmez — bkz. qtss.conf.example son satırları.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${REPO_ROOT}/deploy/nginx/qtss.conf.example"
DST="/etc/nginx/conf.d/qtss.conf"
SERVER_NAME="${SERVER_NAME:-_}"

if [[ ! -f "${SRC}" ]]; then
  echo "missing ${SRC}"
  exit 1
fi

if (( EUID != 0 )); then
  echo "must run as root (use sudo)"
  exit 1
fi

if ! command -v nginx >/dev/null 2>&1; then
  echo "Installing nginx..."
  dnf install -y nginx
fi

echo "Rendering ${DST} (server_name=${SERVER_NAME})..."
sed "s|server_name _;.*|server_name ${SERVER_NAME};|" "${SRC}" > "${DST}"

# Disable the stock catch-all server so our conf.d entry wins on :80.
# RHEL ships /etc/nginx/nginx.conf with a `server { listen 80 default_server; ... }`
# block — we comment it out exactly once and leave a marker so a re-run
# is a no-op.
NGINX_MAIN=/etc/nginx/nginx.conf
if [[ -f "${NGINX_MAIN}" ]] && ! grep -q '# QTSS_DEFAULT_SERVER_DISABLED' "${NGINX_MAIN}"; then
  if grep -qE 'listen\s+80\s+default_server' "${NGINX_MAIN}"; then
    cp -a "${NGINX_MAIN}" "${NGINX_MAIN}.bak.qtss"
    awk '
      BEGIN { depth = 0; in_block = 0 }
      /server\s*\{/ && in_block == 0 {
        # Peek the next ~10 lines for default_server. We use a buffer.
      }
      { print }
    ' "${NGINX_MAIN}" > /dev/null  # placeholder; real edit below
    # Simple, conservative edit: comment out the canonical block.
    python3 - "${NGINX_MAIN}" <<'PY'
import re, sys
p = sys.argv[1]
src = open(p).read()
pat = re.compile(r'(\n\s*server\s*\{[^}]*?listen\s+80\s+default_server.*?\n\s*\})', re.DOTALL)
new, n = pat.subn(lambda m: '\n# QTSS_DEFAULT_SERVER_DISABLED\n' + '\n'.join('# ' + l for l in m.group(1).strip('\n').split('\n')), src)
if n:
    open(p, 'w').write(new)
PY
  fi
fi

echo "nginx -t..."
nginx -t

echo "Enabling + reloading nginx..."
systemctl enable --now nginx
systemctl reload nginx

echo "Done. QTSS reverse proxy active on :80 (server_name=${SERVER_NAME})."
echo "Verify: curl -sI http://localhost/api/v1/health"
