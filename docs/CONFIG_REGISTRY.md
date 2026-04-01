# QTSS — Yapılandırma kayıt defteri (`system_config` / `app_config`)

Bu dosya FAZ **11.3** ile uyumludur. Bu repoda `system_config` şeması **`0044_system_config.sql`** ile gelir; geniş tüm idempotent tohumlar **`0058_system_config_runtime_seeds.sql`**. Her `NNNN_*.sql` sürüm numarası **yalnızca bir** dosyada kullanılmalıdır (`ls migrations/0013*.sql` tek satır olmalı).

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

### `notify` — Telegram (AI onayı)

| `config_key` | `value` örneği | `is_secret` | Açıklama |
|--------------|----------------|-------------|-----------|
| `telegram_bot_token` | `{"value":"123456:ABC..."}` | `true` | BotFather token; worker/API bildirimi + webhook cevapları |
| `telegram_chat_id` | `{"value":"-1001234567890"}` | `false` | Hedef sohbet/kanal |
| `telegram_webhook_secret` | `{"value":"random-long-hex"}` | `true` | Public URL parçası: `POST /telegram/webhook/{secret}` |

`notify.dispatcher_config` içindeki `telegram` nesnesi hâlâ geçerlidir; **hem** token/chat **tablo satırları** doluysa bu satırlar önceliklidir (`qtss_ai::load_notify_config_merged`). Webhook: `https://<api-host>/telegram/webhook/<telegram_webhook_secret>` (BotFather `setWebhook`). Inline düğme `callback_data`: **`d:{uuid}:a|r`** → `ai_decisions` (worker/API bildirimi), **`a:{uuid}:a|r`** → `ai_approval_requests` (POST `/api/v1/ai/approval-requests` sonrası API’den gönderilen mesaj). `a:` ile çözülen onayda, `ai_decisions.approval_request_id` eşleşen bekleyen kararlar da aynı oturumda onaylanır / reddedilir (`mirror_approval_request_outcome_to_linked_ai_decisions`).

**Worker — Trading Range / sweep (`qtss-analysis`):** `notify.notify_on_range_events` → `{"enabled":true}` (veya `QTSS_NOTIFY_ON_RANGE_EVENTS=1`; öncelik: `QTSS_CONFIG_ENV_OVERRIDES=1` iken env) ve kanal listesi `notify_on_range_events_channels` / `QTSS_NOTIFY_ON_RANGE_EVENTS_CHANNELS` (varsayılan `telegram`). Telegram ve diğer kanallar: `notify.dispatcher_config` + `telegram_bot_token` / `telegram_chat_id` (`qtss_ai::load_notify_config_merged`). Aynı şema sweep için: `notify_on_sweep`, `notify_on_sweep_channels` (varsayılan `webhook`). Gönderilen olaylar: `signal_dashboard` **DONUS** kenarı; `range_signal_events` için yeni `long_entry` / `short_entry` / `long_exit` / `short_exit` (DB `ON CONFLICT` ile tekrar yazılmaz, bildirim tek sefer).

**Worker — `notify_outbox`:** Her turda `qtss_ai::load_notify_config_merged` (outbox da `telegram_*` satırlarını kullanır).

## Admin API

- `GET/POST/DELETE /api/v1/admin/system-config` — `?module=` filtresi; **admin** rolü.

## PR checklist

1. Yeni tick veya bayrak: hem **env yedeği** (`QTSS_*`) hem mümkünse **`system_config` seed** (idempotent migration).
2. `docs/QTSS_MASTER_DEV_GUIDE.md` Bölüm **0** / **FAZ 11** özet tablosunu güncelle.
3. `migrations/README.md` envanter satırı ekleyin.

---

*Minimal repo kopyalarında bu dosya yol gösterir; şema yoksa önce tam migration zincirini uygulayın.*
