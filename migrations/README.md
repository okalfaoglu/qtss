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

## Rules (summary)

- One numeric prefix per file; never edit an already-applied migration — add a new file.
- After adding a file, update this README table and run `qtss-sync-sqlx-checksums` if your workflow uses offline SQLx query data.
