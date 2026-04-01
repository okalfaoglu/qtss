# SQL migrations (`migrations/*.sql`)

Applied at API/worker startup via `qtss_storage::run_migrations` (SQLx).

## Inventory (this checkout)

Source of truth:

```bash
ls migrations/*.sql | sort
```

**Count:** (see command above). Refresh with:

```bash
ls migrations/*.sql | sort
```

## Full deployment line (extended schema)

Downstream / full-tree clones may continue beyond this minimal inventory (AI tables, notify outbox, extended worker schemas, etc.). The master guide (`docs/QTSS_MASTER_DEV_GUIDE.md`) and `docs/CONFIG_REGISTRY.md` describe that chain. **Do not renumber** existing applied migrations; add the next free prefix only.

**This checkout — `0013`:** `ai_approval_requests`, `ai_decisions` (+ `approval_request_id`), taktik/operasyonel/portföy/yönüm tabloları. CI: `cargo test -p qtss-storage --test migrations_apply` (`DATABASE_URL`).

**Çakışma:** Veritabanında zaten başka bir PR’den `ai_decisions` / `0042_*` zinciri varsa **`0013` uygulama** — yalnızca `ALTER TABLE ai_decisions ADD COLUMN approval_request_id ...` gibi ek bir migration ile genişletin; tabloları ikinci kez `CREATE` etmeyin.

## Rules (summary)

- One numeric prefix per file; never edit an already-applied migration — add a new file.
- After adding a file, update this README table and run `qtss-sync-sqlx-checksums` if your workflow uses offline SQLx query data.
