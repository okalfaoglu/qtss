# SQLx PostgreSQL migrations

- **Dosya adı:** `NNNN_short_description.sql` (İngilizce slug) — SQLx sürümü = `NNNN` (örn. `0034` → `_sqlx_migrations.version = 34`).
- **Tek dosya / sürüm:** Aynı `NNNN` önekinden iki `.sql` olamaz (`qtss-sync-sqlx-checksums` ve `migrate` kırılır).
- **Tam liste:** Bu klasör geliştirme / CI / üretim ile **aynı** olmalı; `ls *.sql | sort` farklıysa repoyu senkronlayın.
- **Dokümantasyon:** `docs/QTSS_CURSOR_DEV_GUIDE.md` §6 (numaralandırma, 29–35 örnek tablosu), `docs/PROJECT.md` §7 (şema özeti). Çift önek düzeltmesi: tek `0013_worker_analytics_schema.sql`, tek `0014_catalog_fk_columns.sql`. `0034` / `0035`: `engine_symbols` FK; `bar_intervals` yoksa 0034 `bar_interval_id` eklemez, tablo sonradan gelirse 0035 tamamlar.
