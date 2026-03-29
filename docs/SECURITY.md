# QTSS — Güvenlik taslağı

Üretim öncesi kontrol listesi ve gizli anahtar stratejisi özeti.

## Ağ ve API

- **TLS**: API ve web yalnızca HTTPS üzerinden yayınlanmalı; sertifika yenileme otomatik (ör. Let’s Encrypt, Ingress TLS).
- **Rate limit**: Uygulama içi `tower-governor` eşler IP; CDN / WAF / nginx ile ek sınır ve bağlantı sınırları önerilir.
- **Ters vekil**: `QTSS_TRUSTED_PROXIES` yalnızca güvenilen vekil ağlarını listelemeli. Aksi halde `X-Forwarded-For` sahteciliği rate limit atlamasına yol açar.
- **Metrikler**: `QTSS_METRICS_TOKEN` üretimde doldurulmalı; `/metrics` iç ağ veya Bearer ile korunmalı.
- **Probe uçları**: `GET /live` ve `GET /ready` kimlik doğrulaması istemez (kube / LB için); yalnızca iç ağ veya Ingress kısıtlaması ile kullanın; `/ready` DB erişimini doğrular. Worker’da aynı yollar `QTSS_WORKER_HTTP_BIND` ile açılır (ayrı port).

## Kimlik bilgileri

- **`QTSS_JWT_SECRET`**: Güçlü rastgele (≥32 byte); döndürme politikası ve ortam başına ayrı secret.
- **OAuth istemcileri**: `oauth_clients.client_secret` hash’li (mevcut); istemci başına grant kısıtları.
- **Borsa API anahtarları** (`exchange_accounts`):
  - *Geliştirme*: Düz metin kabul edilebilir.
  - *Hedef*: **HashiCorp Vault**, AWS Secrets Manager, GCP Secret Manager veya benzeri; uygulama yalnızca kısa ömürlü okuma.
  - *Uygulama*: Anahtarları alan servis Vault’tan çözer, DB’de sadece **referans** (path / version) tutulabilir; veya alan düzeyinde uygulama içi AEAD (ör. KMS ile data key).

## Denetim

- **`audit_log`**: `/api/v1/*` üzerindeki mutasyonlar yalnızca **`QTSS_AUDIT_HTTP=1`** iken kaydedilir; değişken tanımsız veya `1` dışındaysa denetim kapalıdır. Saklama süresi ve PII politikası operasyonel karar.
- İleride: imzalı denetim zinciri, ayrı immutability (WORM) storage.

## Tenancy

- JWT `org_id` ile hizalanan satır düzeyi kontrolleri; yönetici raporlarında çok kiracı sızıntısına karşı testler.

## Bağımlılık ve tedarik

- **sqlx**: Kök `Cargo.toml` içinde `default-features = false` ve yalnızca `postgres` (+ `runtime-tokio-rustls`, `macros`, …). `macros` zinciri (`sqlx-macros-core`) yine de kilit dosyasına `sqlx-mysql` ve **RUSTSEC-2023-0071** (`rsa` / Marvin) ekler; QTSS çalışma zamanında MySQL kullanmaz. Depoda **`.cargo/audit.toml`** bu danışmanlığı bilinçli yok sayar; sqlx tarafında makro bağımlılığı kalkana kadar `cargo audit` bunun dışında temiz kalmalıdır. `Cargo.lock`, `Cargo.toml` ile uyumlu olmalı.
- Düzenli `cargo audit` / benzeri; imaj taraması (container). Yerelde **cargo-audit ≥ 0.22** kullanın: RustSec veritabanındaki `CVSS:4.0` vektörlerini 0.21.x okuyamaz (`unsupported CVSS version: 4.0`). Güncelleme: `cargo install cargo-audit --version 0.22.1 --locked` (veya daha yeni patch).
- **CI:** `.github/workflows/rust-ci.yml` — push/PR’de `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo audit` (sabitlenmiş `cargo-audit@0.22.1`), `web` için `npm ci` + `npm run build`.

## Worker — otomatik emir

- **`QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED`**: `qtss-worker` içinde SL/TP eşiğinde Binance’a **gerçek** reduce-only / satış emri gönderebilir; `exchange_accounts` düz metin anahtar okur. Üretimde varsayılan **kapalı** tutulmalı; açmadan önce dry yol (`QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED`) ve risk onayı önerilir. Ayrıntı: `docs/QTSS_CURSOR_DEV_GUIDE.md` §3.5, ADIM 9, §10 SSS.

Bu belge mimari hedefleri tanımlar; güvenlik onayı için kurum içi süreçlere tabidir.
