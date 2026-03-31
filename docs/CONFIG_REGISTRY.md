# QTSS — Yapılandırma kayıt defteri (`system_config` / `app_config`)

Bu dosya FAZ **11.3** ile uyumludur. Bu repoda `system_config` şeması migration **`0044_system_config.sql`** ile gelir.

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

## Admin API

- `GET/POST/DELETE /api/v1/admin/system-config` — `?module=` filtresi; **admin** rolü.

## PR checklist

1. Yeni tick veya bayrak: hem **env yedeği** (`QTSS_*`) hem mümkünse **`system_config` seed** (idempotent migration).
2. `docs/QTSS_MASTER_DEV_GUIDE.md` Bölüm **0** / **FAZ 11** özet tablosunu güncelle.
3. `migrations/README.md` envanter satırı ekleyin.

---

*Minimal repo kopyalarında bu dosya yol gösterir; şema yoksa önce tam migration zincirini uygulayın.*
