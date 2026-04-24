# QTSS Consolidated Migrations

Bu dizin **yeni bir QTSS kurulumunu** 233 incremental migration yerine **iki** baseline dosyası ile ayağa kaldırmak için hazırlanmıştır. Production'da (zaten 233 migration çalıştırılmış kurulumlarda) hiçbir değişiklik gerektirmez — `migrations/` dizini dokunulmadan kalır ve `_sqlx_migrations` tablosu eski kayıtlarla devam eder.

## Dosyalar

| Dosya | Kaynak | Satır | İçerik |
|---|---|---|---|
| `0001_baseline_schema.sql` | `pg_dump --schema-only` | 5,314 | Tüm CREATE TABLE / CREATE INDEX / CREATE FUNCTION / TimescaleDB hypertable'ları |
| `0002_baseline_config_seed.sql` | `pg_dump --data-only --table=system_config` | 1,089 | Tüm `system_config` satırları (detector threshold'ları, feature flag'ler, weight tabloları) |

## Yeni Kurulum Akışı

```bash
# 1. Boş DB oluştur
psql -c "CREATE DATABASE qtss;"

# 2. Baseline schema + seed uygula
psql -d qtss -f migrations_consolidated/0001_baseline_schema.sql
psql -d qtss -f migrations_consolidated/0002_baseline_config_seed.sql

# 3. sqlx migration tablosunu "baseline'a kadar zaten çalıştırıldı" olarak işaretle
#    (bu sayede `sqlx migrate run` mevcut 233 dosyayı yeniden çalıştırmaz)
psql -d qtss -c "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
                 SELECT 1000, 'baseline_consolidated_v1', now(), true,
                        '\x'::bytea, 0;"
```

## Production Sistemlerinde

**Değiştirilecek hiçbir şey yok.** Eski `migrations/` dizini yerinde kalır; `_sqlx_migrations` 233 satır içerir. Yeni incremental migration'lar yine `migrations/0234_...sql` şeklinde eklenir.

## Baseline'ı Güncellemek

Yeni migration'lar biriktiğinde (örneğin 233 → 300) bir sonraki baseline resync için:

```bash
# Ham schema dump
pg_dump -U qtss -d qtss --schema-only --no-owner --no-acl --no-comments \
  -T '_elliott_purge_queue' -N '_timescaledb_*' -N 'timescaledb_*' \
  > migrations_consolidated/0001_baseline_schema.sql

# system_config seed
pg_dump -U qtss -d qtss --data-only --no-owner --table=public.system_config \
  --column-inserts > migrations_consolidated/0002_baseline_config_seed.sql
```

## Kararlar & Dışarıda Bırakılanlar

- **`_elliott_purge_queue`** (legacy demolition artifact) dump'tan çıkarıldı — yeni kurulumlarda bu tabloya ihtiyaç yok.
- **TimescaleDB internal schema'ları** (`_timescaledb_*`) dışarıda bırakıldı — extension kendi yapısını oluşturur.
- **Domain verileri** (market_bars, pivots, detections, vs.) dahil değil — bunlar runtime'da yazılır. Yeni kurulum boş DB ile başlar.
- **AI prompt template'leri, user accounts** gibi organizasyon-specific seed'ler baseline'da yok — her organizasyon kendi `00N_org_seed.sql`'ini yazar.

## Arşivleme Planı

Eski `migrations/` dizinini tamamen `migrations/_archive/` altına taşımak isteniyorsa:

```bash
mkdir -p migrations/_archive
git mv migrations/0001_*.sql migrations/0002_*.sql ... migrations/0233_*.sql migrations/_archive/
```

**Bu taşıma henüz yapılmadı** — production sistemleri zaten bu path'den okuduğu için kırılganlık yaratır. Commit edilmesi için sunucunun kesin kapatıldığı bir maintenance window gerekir.
