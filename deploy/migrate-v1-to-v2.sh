#!/usr/bin/env bash
# QTSS prod migration helper — v1 → v2 cutover.
#
# İki aşamalı:
#   STEP=install   → web-v2 unit'i kur, build et, başlat. v1 dokunulmaz.
#                    (Yan yana çalışır; nginx hâlâ /'i v1'e gönderir.)
#   STEP=cutover   → nginx'i /'i web-v2'ye çevirecek şekilde yeniden yaz,
#                    reload et. v1 hâlâ ayakta ama trafik almaz.
#   STEP=remove    → v1 servislerini durdur, disable et, unit dosyalarını sil,
#                    web/ build çıktısını kaldır. (Geri dönüş yok — önce cutover'ı doğrula.)
#
# Kullanım:
#   sudo STEP=install ./deploy/migrate-v1-to-v2.sh
#   # smoke test: curl -I http://localhost/v2/  &&  curl -I http://localhost/api/health
#   sudo STEP=cutover ./deploy/migrate-v1-to-v2.sh
#   # 1-2 gün gözle, sorun yoksa:
#   sudo STEP=remove  ./deploy/migrate-v1-to-v2.sh
#
# Ortam:
#   QTSS_ROOT       — repo kökü (default: /app/qtss)
#   NGINX_CONF      — nginx config yolu (default: /etc/nginx/conf.d/qtss.conf)
#   SYSTEMD_DIR     — systemd unit dizini (default: /etc/systemd/system)
set -euo pipefail

ROOT="${QTSS_ROOT:-/app/qtss}"
NGINX_CONF="${NGINX_CONF:-/etc/nginx/conf.d/qtss.conf}"
SYSTEMD_DIR="${SYSTEMD_DIR:-/etc/systemd/system}"
STEP="${STEP:-}"

log() { printf '==> %s\n' "$*"; }
die() { printf 'ERR: %s\n' "$*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "root gerekiyor (sudo ile çalıştır)"
[[ -d "$ROOT" ]] || die "repo bulunamadı: $ROOT"
cd "$ROOT"

case "$STEP" in
  install)
    log "web-v2 build"
    [[ -d web-v2 ]] || die "web-v2/ dizini yok"
    if [[ -f web-v2/package-lock.json ]]; then
      (cd web-v2 && npm ci && npm run build)
    else
      (cd web-v2 && npm install && npm run build)
    fi

    log "qtss-web-v2.service kur"
    cp deploy/systemd/qtss-web-v2.service.example "$SYSTEMD_DIR/qtss-web-v2.service"
    sed -i "s|/app/qtss|$ROOT|g" "$SYSTEMD_DIR/qtss-web-v2.service"
    log "DİKKAT: $SYSTEMD_DIR/qtss-web-v2.service içindeki DATABASE_URL parolasını düzenle, sonra Enter'a bas."
    read -r _

    systemctl daemon-reload
    systemctl enable --now qtss-web-v2.service
    sleep 2
    systemctl --no-pager status qtss-web-v2.service | head -15

    log "nginx config'i yan yana moda al (yoksa kopyala)"
    if [[ ! -f "$NGINX_CONF" ]]; then
      cp deploy/nginx/qtss.conf.example "$NGINX_CONF"
      log "DİKKAT: $NGINX_CONF içinde server_name'i düzenle, sonra Enter'a bas."
      read -r _
    fi
    nginx -t && systemctl reload nginx

    log "install tamam. Smoke test:"
    log "  curl -I http://localhost/        # v1 (200)"
    log "  curl -I http://localhost/v2/     # v2 (200)"
    log "  curl    http://localhost/api/health"
    ;;

  cutover)
    log "nginx config: / → web-v2 (4174), /v1/ → web v1 (4173)"
    [[ -f "$NGINX_CONF" ]] || die "$NGINX_CONF yok — önce STEP=install"
    cp "$NGINX_CONF" "${NGINX_CONF}.bak.$(date +%s)"
    cat > "$NGINX_CONF" <<'NGINX'
upstream qtss_api    { server 127.0.0.1:8080; }
upstream qtss_web_v1 { server 127.0.0.1:4173; }
upstream qtss_web_v2 { server 127.0.0.1:4174; }

server {
    listen 80;
    server_name _;
    client_max_body_size 16m;

    location /api/ {
        proxy_pass         http://qtss_api/;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;
        proxy_read_timeout 120s;
    }

    location ~ ^/(health|live|ready|metrics)$ {
        proxy_pass http://qtss_api;
        proxy_set_header Host $host;
    }

    # Legacy v1 — yedek olarak /v1/ altında erişilebilir
    location /v1/ {
        proxy_pass         http://qtss_web_v1/;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   Upgrade           $http_upgrade;
        proxy_set_header   Connection        "upgrade";
    }

    # Yeni varsayılan: web v2
    location / {
        proxy_pass         http://qtss_web_v2;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Forwarded-Proto $scheme;
        proxy_set_header   Upgrade           $http_upgrade;
        proxy_set_header   Connection        "upgrade";
    }
}
NGINX
    nginx -t && systemctl reload nginx
    log "cutover tamam. Trafik artık v2'de. v1 hâlâ ayakta (rollback için)."
    log "rollback: $NGINX_CONF.bak.* dosyasını geri kopyala + nginx reload"
    ;;

  remove)
    log "v1 servislerini durdur ve disable et"
    for u in qtss-web; do
      if systemctl cat "${u}.service" &>/dev/null; then
        systemctl disable --now "${u}.service" || true
        rm -f "$SYSTEMD_DIR/${u}.service"
        log "  silindi: ${u}.service"
      fi
    done
    systemctl daemon-reload

    log "web/ build çıktısını temizle (kaynağı bırakıyoruz, sadece dist/node_modules)"
    rm -rf "$ROOT/web/dist" "$ROOT/web/node_modules" || true

    log "nginx config'inden /v1/ location bloğunu manuel kaldır (gerekiyorsa):"
    log "  sudo nano $NGINX_CONF   # /v1/ bloğunu sil"
    log "  sudo nginx -t && sudo systemctl reload nginx"

    log "remove tamam. v1 artık çalışmıyor."
    log "İsteğe bağlı: web/ kaynak dizinini de silmek istiyorsan:  rm -rf $ROOT/web"
    ;;

  *)
    cat <<USAGE
Kullanım: sudo STEP=<install|cutover|remove> $0

  install  — web-v2 build + qtss-web-v2.service + nginx yan yana mod
  cutover  — nginx / → web v2, /v1/ → web v1 (v1 hâlâ ayakta)
  remove   — v1 systemd unit'i sil, web/ build çıktısını temizle

Sıra: install → smoke test → cutover → 1-2 gün gözle → remove
USAGE
    exit 2
    ;;
esac
