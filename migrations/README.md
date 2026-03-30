# SQL migrations (`migrations/*.sql`)

Applied at API/worker startup via `qtss_storage::run_migrations` (SQLx).

## Inventory (this checkout)

| Version | File |
|--------:|------|
| 1 | `0001_init.sql` |
| 2 | `0002_oauth.sql` |
| 3 | `0003_market_catalog.sql` |
| 4 | `0004_exchange_orders.sql` |
| 5 | `0005_audit_log.sql` |
| 6 | `0006_market_bars.sql` |
| 7 | `0007_acp_chart_patterns.sql` |
| 8 | `0008_acp_zigzag_seven_fib.sql` |
| 9 | `0009_acp_pine_indicator_defaults.sql` |
| 10 | `0010_acp_abstract_size_filters.sql` |
| 11 | `0011_acp_last_pivot_direction.sql` |
| 12 | `0012_acp_pattern_groups.sql` |

**Count:** 12 files. Refresh with:

```bash
ls migrations/*.sql | sort
```

## Full deployment line (AI + `system_config` + worker seeds)

Downstream / full-tree clones may continue through **`0042`–`0047`** (AI tables, `system_config`, locale ticks, kill-switch ticks, etc.). The master guide (`docs/QTSS_MASTER_DEV_GUIDE.md` §5, FAZ 1 / 11) and `docs/CONFIG_REGISTRY.md` describe that chain. **Do not renumber** existing applied migrations; add the next free prefix only.

## Rules (summary)

- One numeric prefix per file; never edit an already-applied migration — add a new file.
- After adding a file, update this README table and run `qtss-sync-sqlx-checksums` if your workflow uses offline SQLx query data.
