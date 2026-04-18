#!/usr/bin/env bash
# QTSS one-shot deploy: rebuild rust, refresh web deps, restart all services,
# print status + health probes.
#
# Usage:
#   ./scripts/qtss-deploy.sh                # full: build + restart all
#   ./scripts/qtss-deploy.sh --no-build     # just restart services
#   ./scripts/qtss-deploy.sh --only web     # restart single service
#                                           # (web|api|worker|inference)
#
# Services managed: qtss-worker, qtss-api, qtss-web, qtss-inference
set -euo pipefail

REPO=/app/qtss
SERVICES=(qtss-worker qtss-api qtss-web qtss-inference)
BUILD=1
ONLY=

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=0; shift ;;
    --only)     ONLY="qtss-$2"; shift 2 ;;
    -h|--help)  sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
done

cd "$REPO"

if [[ $BUILD -eq 1 && -z "$ONLY" ]]; then
  echo '[deploy] cargo build --release (qtss-worker + qtss-api)'
  cargo build --release -p qtss-worker -p qtss-api
  echo '[deploy] npm install (web-v2)'
  (cd web-v2 && npm install --no-audit --no-fund --silent)
  echo '[deploy] pip sync (trainer sidecar)'
  /app/qtss/trainer/.venv/bin/pip install -q -e /app/qtss/trainer || true
fi

if [[ -n "$ONLY" ]]; then
  TARGETS=("$ONLY")
else
  TARGETS=("${SERVICES[@]}")
fi

echo "[deploy] restarting: ${TARGETS[*]}"
sudo systemctl restart "${TARGETS[@]}"

sleep 3
echo '[deploy] active status:'
systemctl is-active "${SERVICES[@]}" || true

echo '[deploy] health probes:'
curl -sS -o /dev/null -w 'api    :8080 = %{http_code}\n' http://127.0.0.1:8080/healthz || true
curl -sS -o /dev/null -w 'web    :5174 = %{http_code}\n' http://127.0.0.1:5174/      || true
curl -sS -o /dev/null -w 'infer  :8790 = %{http_code}\n' http://127.0.0.1:8790/health || true

echo '[deploy] done.'
