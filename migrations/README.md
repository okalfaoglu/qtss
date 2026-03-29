# SQLx PostgreSQL migrations

- **Dosya adı:** `NNNN_short_description.sql` (İngilizce slug) — SQLx sürümü = `NNNN` (örn. `0034` → `_sqlx_migrations.version = 34`).
- **Tek dosya / sürüm:** Aynı `NNNN` önekinden iki `.sql` olamaz (`qtss-sync-sqlx-checksums` ve `migrate` kırılır).
- **Tam liste:** Bu klasör geliştirme / CI / üretim ile **aynı** olmalı; aşağıdaki envanterdeki **`.sql` satır sayısı** `ls migrations/*.sql | wc -l` ve `git ls-files 'migrations/*.sql' | wc -l` ile eşleşmeli (tam repoda şu an **0001–0036**, 36 dosya).
- **Dokümantasyon / kesin kural:** `docs/QTSS_CURSOR_DEV_GUIDE.md` §0, §4 (ADIM ↔ kod), §5 (ortam tek kaynak: kök `.env.example`), §6 (numaralandırma, 29–36 tablo), `docs/PROJECT.md` §7; migrasyon hata metni: `crates/qtss-storage/src/pool.rs`. Çift önek düzeltmesi: tek `0013_worker_analytics_schema.sql`, tek `0014_catalog_fk_columns.sql`. `0034` / `0035`: `engine_symbols` FK; `0036_bar_intervals_repair_if_missing.sql`: `bar_intervals` yoksa oluşturur (bozuk 0013 telafisi).

## Inventory (`*.sql` — tam repoda 36 dosya, 0001–0036)

Sıra `sqlx::migrate!` uygulama sırası ile aynı olmalı (`sort`).

```
0001_init.sql
0002_oauth.sql
0003_market_catalog.sql
0004_exchange_orders.sql
0005_audit_log.sql
0006_market_bars.sql
0007_acp_chart_patterns.sql
0008_acp_zigzag_seven_fib.sql
0009_acp_pine_indicator_defaults.sql
0010_acp_abstract_size_filters.sql
0011_acp_last_pivot_direction.sql
0012_acp_pattern_groups.sql
0013_worker_analytics_schema.sql
0014_catalog_fk_columns.sql
0015_engine_analysis.sql
0016_range_signal_events.sql
0017_paper_ledger.sql
0018_engine_signal_direction_mode.sql
0019_nansen_snapshots.sql
0020_nansen_setup_scans.sql
0021_external_data_fetch.sql
0022_data_snapshots_confluence.sql
0023_external_data_sources_seed_f7.sql
0024_drop_external_data_snapshots.sql
0025_confluence_weights_app_config.sql
0026_external_source_hl_meta_asset_ctxs.sql
0027_market_confluence_snapshots.sql
0028_external_sources_funding_oi_liquidations.sql
0029_market_confluence_payload_column.sql
0030_onchain_signal_scores.sql
0031_onchain_signal_weights_app_config.sql
0032_nansen_extended_scores.sql
0033_onchain_weights_hl_whale.sql
0034_engine_symbols_fk_columns.sql
0035_engine_symbols_bar_interval_fk_if_ready.sql
0036_bar_intervals_repair_if_missing.sql
```

Yeni migration: bir sonraki boş numara **`0037_*.sql`** (veya dizindeki en yüksek önek + 1).
