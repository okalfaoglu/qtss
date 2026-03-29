# Nansen — Token Screener (operatör rehberi)

Resmi API dokümantasyonu: [docs.nansen.ai](https://docs.nansen.ai/).

## Akış (qtss-worker)

- Sunucuda yalnızca worker ortamında **`NANSEN_API_KEY`** tanımlanır. Anahtar **tarayıcıya**, **Vite `.env`** dosyasına ve **git reposuna** konmamalıdır.
- Worker, `POST {NANSEN_API_BASE}/api/v1/token-screener` çağrısını yapar (HTTP istemcisi ve gövde: `crates/qtss-nansen/src/lib.rs`, `crates/qtss-worker/src/nansen_engine.rs`).
- Başarılı yanıt **`nansen_snapshots`** tablosunda `snapshot_kind = token_screener` satırına yazılır (tek satır upsert). Confluence / birleşik okuma için aynı veri **`data_snapshots.source_key = nansen_token_screener`** olarak da güncellenir (`crates/qtss-worker/src/data_sources/registry.rs`).

## Ortam değişkenleri

| Değişken | Açıklama |
|----------|-----------|
| `NANSEN_API_KEY` | Zorunlu (worker). Boşsa döngü uyarı verir, istek atmaz. |
| `NANSEN_API_BASE` | Varsayılan `https://api.nansen.ai`. |
| `NANSEN_TICK_SECS` | İki çağrı arası bekleme (sn). Varsayılan **1800** (30 dk); kredi için yüksek tutun. |
| `NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS` | **403** ve “insufficient credits” sonrası bir sonraki denemeden önce bekleme (sn). Varsayılan **3600**; en az `NANSEN_TICK_SECS` kadar. |
| `NANSEN_TOKEN_SCREENER_REQUEST_JSON` | İsteğe bağlı **tam** istek gövdesi (JSON string). Yoksa sırayla: **`app_config`** (`QTSS_NANSEN_CONFIG_KEY`, varsayılan `nansen_screener_request`) → bu env → kod varsayılanı (`crates/qtss-worker/src/nansen_query.rs`). |
| `QTSS_NANSEN_CONFIG_KEY` | Admin `PUT /api/v1/config` ile farklı preset anahtarı (ör. `nansen_screener_request_v2`). |

## Setup taraması (ikinci aşama)

- **`setup_scan_engine`** (`crates/qtss-worker/src/setup_scan_engine.rs`): token screener satırları + dahili skor → **`nansen_setup_runs`** / **`nansen_setup_rows`** (migration sürümü kod yorumlarında **0020** olarak geçer; şema güncellemeleri repo `migrations/` altında).
- Çıktı: en iyi **5 LONG** + **5 SHORT** (ayrı sıralı; toplam en fazla **10** satır).
- **`QTSS_SETUP_SCAN_SECS`**: tarama aralığı (varsayılan **900** sn).
- **`QTSS_SETUP_SNAPSHOT_ONLY`**: Varsayılan **1** (açık): setup **ikinci bir Nansen HTTP isteği yapmaz**, yalnızca `nansen_snapshots` okur; kredi tek yerde (`nansen_engine`). Canlı yedek (ek Nansen çağrısı): **`0`** / `false` / `no` / `off`.
- **`QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS`**: Yalnızca **`QTSS_SETUP_SNAPSHOT_ONLY=0`** iken canlı yedek kararında kullanılır (eskimiş snapshot + bu modda tekrar API çağrısı).

Bildirim özeti: `QTSS_NOTIFY_SETUP_*` — bkz. kök `.env.example` ve `docs/PLAN_CONFLUENCE_AND_MARKET_DATA.md` Phase D.

## HTTP API (qtss-api, JWT)

Dashboard / analist rolleri ile:

- **`GET /api/v1/analysis/nansen/snapshot`** — son `token_screener` snapshot’ı (`request_json`, `response_json`, `meta_json`, `computed_at`, `error`).
- **`GET /api/v1/analysis/nansen/setups/latest`** — son setup koşusu + sıralı satırlar (`run` + `rows`).

Kod: `crates/qtss-api/src/routes/analysis.rs`.

Web istemcisi: `web/src/api/client.ts` (`fetchNansenSnapshot`, `fetchNansenSetupsLatest`).

## Web: `VITE_API_BASE`

- Yalnızca **kök origin** verin (örn. `http://127.0.0.1:8080`). Sonuna **`/api/v1` eklemeyin** — istemci yolları zaten `/api/v1/...` ile birleştirilir; çift önek **404** üretir. Ayrıntı: `web/.env.example`.

## Sorun giderme

| Belirti | Olası neden |
|---------|-------------|
| **403** / “Insufficient credits” | Nansen hesap kredisi bitti. `NANSEN_TICK_SECS` artırın veya planı yükseltin. Worker, `NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS` ile daha seyrek dener. |
| **`GET .../nansen/setups/latest` → 404** | Sunucudaki **qtss-api** eski binary (bu rota yok). Güncel kodla derleyip yeniden başlatın. |
| Snapshot’ta `error` dolu, `response_json` null | Son çağrı başarısız; gövde API hata metnini içerir. |

## Örnek: başarılı çağrı meta + gövde + yanıt

`computed_at` ve sayılar örnektir; sizin ortamınızda farklı olur.

**meta_json** (yanıt header’larından türetilir; `qtss-nansen`):

```json
{
  "credits_remaining": "9830",
  "credits_used": "10",
  "rate_limit_remaining": "19",
  "response_token_count_hint": 0
}
```

**request_json** (varsayılan gövde ile uyumlu; `nansen_query::default_token_screener_body`):

```json
{
  "chains": ["ethereum", "solana", "base", "arbitrum", "optimism"],
  "filters": {
    "include_stablecoins": false,
    "liquidity": { "min": 8000 },
    "nof_traders": { "max": 75, "min": 2 },
    "price_change": { "max": 22, "min": -12 },
    "token_age_days": { "max": 150, "min": 1 },
    "trader_type": "sm",
    "volume": { "min": 5000 }
  },
  "order_by": [
    { "direction": "DESC", "field": "volume" },
    { "direction": "DESC", "field": "netflow" }
  ],
  "pagination": { "page": 1, "per_page": 100 },
  "timeframe": "6h"
}
```

**response_json** (filtre sonucu boş olabilir):

```json
{
  "data": [],
  "pagination": {
    "is_last_page": true,
    "page": 1,
    "per_page": 100
  }
}
```

## Snapshot’ı yenilemek

Worker çalışıyor ve `NANSEN_API_KEY` tanımlıysa bir sonraki tick’te otomatik güncellenir. Aralığı geçici kısaltmak için `NANSEN_TICK_SECS` düşürüp servisi yeniden başlatın (kredi tüketimini artırır).

## İlgili plan

[`PLAN_CONFLUENCE_AND_MARKET_DATA.md`](./PLAN_CONFLUENCE_AND_MARKET_DATA.md) — smart money, `data_snapshots`, confluence.
