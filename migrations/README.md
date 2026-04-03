# SQL migrations (`migrations/*.sql`)

Applied at API/worker startup via `qtss_storage::run_migrations` (SQLx).

## Current layout (squashed baseline)

The historical chain (`0001` … `0063`) was merged into a **single** file for simpler deploys and fewer checksum edges:

- **`0001_qtss_baseline.sql`** — full schema + seeds + alters, in the same order as the old numbered files.
- **`0002_notify_telegram_system_config.sql`** — placeholder `notify.telegram_bot_token` / `notify.telegram_chat_id` (empty until configured).
- **`0003_engine_symbol_ingestion_state.sql`** — `engine_symbol_ingestion_state` (worker `market_bars` health per `engine_symbols` row).
- **`0004_worker_engine_ingest_system_config.sql`** — `system_config` seeds for `worker.engine_ingest_*` (tick, min bars, gap window).
- **`0005_api_web_dev_proxy_target.sql`** — `system_config` `api.web_dev_proxy_target` (Vite `/api` `/oauth` `/health` proxy; read when `DATABASE_URL` is set in the Node process).

**Regenerate** (after editing split files in a branch, or restoring from VCS history):

```bash
# From repo root; requires Python 3
python3 scripts/squash_migrations_into_one.py
# Windows (if python3 not on PATH):
py -3 scripts/squash_migrations_into_one.py
```

The script expects at least one `NNNN_*.sql` input **other than** `0001_qtss_baseline.sql`. If only the baseline file exists, it prints instructions and exits successfully (nothing to do).

To re-squash from git history, check out the old `migrations/*.sql` set (move the current baseline aside if it blocks checkout), run the script, then commit.

## Breaking change

Databases that already applied the **old** multi-file chain (`_sqlx_migrations` with versions 2–63) are **not** compatible with this single `0001` baseline (checksum and version sequence differ).

**New environments:** empty database → run API/worker → migration applies once.

**Existing environments:** either keep the pre-squash migration files (restore from git before squash commit) or **drop and recreate** the database and re-seed as needed.

## Rules (summary)

- One numeric prefix per file in the folder (e.g. `0001_*.sql`, `0002_*.sql`, …).
- Do not edit an already-applied migration in production; add a new numbered file or re-squash with a DB reset.
- If your workflow uses offline SQLx query data, refresh it after schema changes (`qtss-sync-sqlx-checksums` / project conventions).
