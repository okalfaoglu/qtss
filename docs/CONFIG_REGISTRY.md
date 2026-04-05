# QTSS — Yapılandırma kayıt defteri (`system_config` / `app_config`)

Bu dosya FAZ **11.3** ile uyumludur. Bu repoda `system_config` DDL + tohumlar tarihsel olarak `0044_*` / `0058_*` migrasyon dosyalarındaydı; **güncel tek dosya:** `migrations/0001_qtss_baseline.sql` içinde `-- >>> merged from: 0044_system_config.sql` ve `0058_system_config_runtime_seeds.sql` bölümleri. Yeni migrasyon eklerken `migrations/README.md` kurallarına bakın.

## `app_config` (mevcut `0001_init`)

- Tek tablo, `key` / `value` (JSONB). Modül kolonu **yok**; anlamsal gruplama anahtar önekleriyle yapılır (`acp_chart_patterns`, `elliott_wave`, `ai_engine_config`, …).
- Admin UI ve API: `GET/POST/DELETE /api/v1/config` (roller: bkz. `docs/PROJECT.md` §8).

## `system_config` (`0044_system_config.sql`)

| Alan | Açıklama |
|------|-----------|
| `module` | Mantıksal sahip: `worker`, `api`, `notify`, `nansen`, `ai`, … |
| `config_key` | Modül içi benzersiz `snake_case` anahtar |
| `value` | JSONB (tercihen nesne: `{"secs":300,"enabled":true}`) |
| `is_secret` | `true` ise list API’de maskeleme; üretimde sırlar aşama A’da env’de kalır |

**Çözümleme:** `qtss_storage::resolve_worker_tick_secs` + `QTSS_CONFIG_ENV_OVERRIDES` + ilgili `QTSS_*` yedekleri. Tam anahtar listesi ve worker modülleri için ana referans: **`docs/QTSS_MASTER_DEV_GUIDE.md` FAZ 11.7** ve üretim repodaki `crates/qtss-storage/src/config_tick.rs`.

### `ai` — qtss-ai LLM sırları ve uçları

| `config_key` | `value` örneği | `is_secret` | Açıklama |
|--------------|----------------|-------------|-----------|
| `anthropic_api_key` | `{"value":""}` | `true` | Anthropic Messages API (`provider_*` = `anthropic`). |
| `gemini_api_key` | `{"value":""}` | `true` | İsteğe bağlı; **boşsa** `telegram_setup_analysis.gemini_api_key` kullanılır (aynı Google AI Studio anahtarı). |
| `gemini_api_root` | `{"value":""}` | `false` | Boş = `https://generativelanguage.googleapis.com/v1beta`. |
| `gemini_timeout_secs` | `{"secs":120}` | `false` | `generateContent` için taban HTTP süresi; istek `suggested_timeout` ile uzar. |

**Sağlayıcı kimlikleri** (`app_config` `ai_engine_config` içindeki `provider_tactical` / `operational` / `strategic`): `anthropic`, `openai_compatible` (OpenAI ChatGPT ve uyumlu uçlar), `ollama`, `gemini` (takma adlar: `google`, `google_gemini`). Kod: `crates/qtss-ai/src/providers/mod.rs` + `provider_secrets.rs`. Eksik satırlar için migrasyon `0002_ai_provider_system_config_ensure.sql`.

### `notify` — Telegram (AI onayı)

| `config_key` | `value` örneği | `is_secret` | Açıklama |
|--------------|----------------|-------------|-----------|
| `telegram_bot_token` | `{"value":"123456:ABC..."}` | `true` | BotFather token; worker/API bildirimi + webhook cevapları |
| `telegram_chat_id` | `{"value":"-1001234567890"}` | `false` | Hedef sohbet/kanal |
| `telegram_webhook_secret` | `{"value":"random-long-hex"}` | `true` | Public URL parçası: `POST /telegram/webhook/{secret}` |

`notify.dispatcher_config` içindeki `telegram` nesnesi hâlâ geçerlidir; **hem** token/chat **tablo satırları** doluysa bu satırlar önceliklidir (`qtss_ai::load_notify_config_merged`). Webhook: `https://<api-host>/telegram/webhook/<telegram_webhook_secret>` (BotFather `setWebhook`). Inline düğme `callback_data`: **`d:{uuid}:a|r`** → `ai_decisions` (worker/API bildirimi), **`a:{uuid}:a|r`** → `ai_approval_requests` (POST `/api/v1/ai/approval-requests` sonrası API’den gönderilen mesaj). `a:` ile çözülen onayda, `ai_decisions.approval_request_id` eşleşen bekleyen kararlar da aynı oturumda onaylanır / reddedilir (`mirror_approval_request_outcome_to_linked_ai_decisions`).

**Worker — Trading Range / sweep (`qtss-analysis`):** `notify.notify_on_range_events` → `{"enabled":true}` (veya `QTSS_NOTIFY_ON_RANGE_EVENTS=1`; öncelik: `QTSS_CONFIG_ENV_OVERRIDES=1` iken env) ve kanal listesi `notify_on_range_events_channels` / `QTSS_NOTIFY_ON_RANGE_EVENTS_CHANNELS` (varsayılan `telegram`). Telegram ve diğer kanallar: `notify.dispatcher_config` + `telegram_bot_token` / `telegram_chat_id` (`qtss_ai::load_notify_config_merged`). Aynı şema sweep için: `notify_on_sweep`, `notify_on_sweep_channels` (varsayılan `webhook`). Gönderilen olaylar: `signal_dashboard` **DONUS** kenarı; `range_signal_events` için yeni `long_entry` / `short_entry` / `long_exit` / `short_exit` (DB `ON CONFLICT` ile tekrar yazılmaz, bildirim tek sefer).

**Worker — `notify_outbox`:** Her turda `qtss_ai::load_notify_config_merged` (outbox da `telegram_*` satırlarını kullanır).

**Worker — `engine_symbol_ingest` (`market_bars` history + gap/stale):**

| `config_key` | `value` örneği | Env yedeği | Açıklama |
|--------------|----------------|------------|-----------|
| `engine_ingest_tick_secs` | `{"secs":180}` | `QTSS_ENGINE_INGEST_TICK_SECS` | Tam tarama aralığı (saniye); `resolve_worker_tick_secs`, min 60. |
| `engine_ingest_min_bars` | `{"value":2000}` | `QTSS_ENGINE_INGEST_MIN_BARS` | Hedef başına minimum mum; REST backfill eşiği; `resolve_system_u64`, clamp 120–50000. |
| `engine_ingest_gap_window` | `{"value":2000}` | `QTSS_ENGINE_INGEST_GAP_WINDOW` | Gap taraması penceresi (son N mum); clamp 100–20000. |

Tohum: `0001_qtss_baseline.sql` içinde `-- >>> merged from: 0004_worker_engine_ingest_system_config.sql` bölümü. Öncelik: `QTSS_CONFIG_ENV_OVERRIDES=1` iken env, aksi halde `system_config`, sonra env, sonra varsayılan (`qtss_storage::config_tick`).

### `api` — Vite dev/preview proxy (Node)

| `config_key` | `value` örneği | Env yedeği | Açıklama |
|--------------|----------------|------------|-----------|
| `web_dev_proxy_target` | `{"value":"http://127.0.0.1:8080"}` | `QTSS_API_PROXY_TARGET` (önce `QTSS_CONFIG_ENV_OVERRIDES=1`) | `npm run dev` / `preview` sırasında `/api`, `/oauth`, `/health` proxy hedefi. Vite sürecinde `DATABASE_URL` gerekir. |

Tohum: `0001_qtss_baseline.sql` içinde `-- >>> merged from: 0005_api_web_dev_proxy_target.sql` bölümü.

## Admin API

- `GET/POST/DELETE /api/v1/admin/system-config` — `?module=` filtresi; **admin** rolü.

## PR checklist

1. Yeni tick veya bayrak: hem **env yedeği** (`QTSS_*`) hem mümkünse **`system_config` seed** (idempotent migration).
2. `docs/QTSS_MASTER_DEV_GUIDE.md` Bölüm **0** / **FAZ 11** özet tablosunu güncelle.
3. `migrations/README.md` envanter satırı ekleyin.

---

*Minimal repo kopyalarında bu dosya yol gösterir; şema yoksa önce tam migration zincirini uygulayın.*
