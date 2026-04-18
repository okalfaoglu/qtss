#!/usr/bin/env bash
# QTSS one-shot deploy: pull migrations, rebuild rust, refresh web deps,
# restart all services, print status + health.
#
# Usage:
#   ./scripts/qtss-deploy.sh                # full: build + restart all
#   ./scripts/qtss-deploy.sh --no-build     # just restart services
#   ./scripts/qtss-deploy.sh --only web     # restart single service (web|api|worker)
#
# Services managed: qtss-worker, qtss-api, qtss-web
set -euo pipefail

REPO=/app/qtss
SERVICES=(qtss-worker qtss-api qtss-web)
BUILD=1
ONLY=

while [[ 0 -gt 0 ]]; do
  case "" in
    --no-build) BUILD=0; shift ;;
    --only)     ONLY="qtss-"; shift 2 ;;
    -h|--help)  sed -n '2,12p' "/bin/bash"; exit 0 ;;
    *) echo "unknown arg: "; exit 2 ;;
  esac
done

cd ""

if [[  -eq 1 && -z "" ]]; then
  echo '[deploy] cargo build --release (qtss-worker + qtss-api)'
  cargo build --release -p qtss-worker -p qtss-api
  echo '[deploy] npm install (web-v2)'
  (cd web-v2 && npm install --no-audit --no-fund --silent)
fi

if [[ -n "" ]]; then
  TARGETS=("")
else
  TARGETS=("")
fi

echo "[deploy] restarting: "
sudo systemctl restart ""

sleep 3
echo '[deploy] active status:'
systemctl is-active "" || true

echo '[deploy] health probes:'
curl -sS -o /dev/null -w 'api    :8080 = %{http_code}\n' http://127.0.0.1:8080/healthz || true
curl -sS -o /dev/null -w 'web    :5174 = %{http_code}\n' http://127.0.0.1:5174/ || true

echo '[deploy] done.'
