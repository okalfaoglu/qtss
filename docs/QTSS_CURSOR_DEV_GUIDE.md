# QTSS — Cursor companion guide

**Purpose:** Stable deep links from source code and operator errors. Narrative status, FAZ tables, and most policies live in [`QTSS_MASTER_DEV_GUIDE.md`](QTSS_MASTER_DEV_GUIDE.md).

**§6 below** is the authoritative place for **SQLx startup failures**, checksum drift, duplicate migration prefixes, and the **squashed baseline** (`migrations/0001_qtss_baseline.sql`) versus older databases.

---

## Legacy section map (code comments still cite old § numbers)

| § cited in code | Where to read |
|-----------------|---------------|
| **§0** | Repo root `.env.example`; environment overview in Master guide **§6** (*Ortam değişkenleri*). |
| **§3.5**, **§10** SSS | `docs/SECURITY.md`; position manager / live close flags; Master guide **§9** (*Güvenlik*). |
| **§4** (ADIM / FAZ) | Master guide **§4** (*Cursor için sıralı görev listesi*). |
| **§5** (PostgreSQL / migrations path) | Master guide **§5** (*Migration kuralları*); layout detail in `migrations/README.md`. |
| **§9.1** (worker tasks) | Master guide **§10** (*Spawn sırası*) and `crates/qtss-worker/src/main.rs`. |

---

## §6 — SQLx migrations, checksum drift, and `_sqlx_migrations`

### What runs migrations

`qtss_storage::run_migrations` (API and worker startup) loads migrations at **runtime** from `./migrations` (relative to process cwd), or `QTSS_MIGRATIONS_DIR`, or `../../migrations` from the `qtss-storage` crate manifest (tests). SQLx records each applied version in **`_sqlx_migrations`** (checksum is **SHA-384** of the migration file bytes on disk).

### Reading API/worker error lines

Startup errors may mention several hints in one line (`bar_intervals`, `0036_…`, duplicate `NNNN_*.sql`). Treat the **`Caused by:`** chain as ground truth: e.g. **`migration 1 was previously applied but has been modified`** means **version 1 checksum mismatch**, not necessarily a missing `bar_intervals` table.

### Symptom: “migration N was previously applied but has been modified”

SQLx compared the checksum stored in `_sqlx_migrations` for version **N** with the checksum of the file **N** on disk; they differ.

**Typical causes**

1. **The `.sql` file for version N was edited** after it was already applied (including reformatting or comments) — violates the rule “never change an applied migration”.
2. **Two files share the same numeric prefix** (e.g. `0001_foo.sql` and `0001_bar.sql`). SQLx expects **exactly one** file per version; the helper `qtss-sync-sqlx-checksums` refuses to run if duplicates exist.
3. **Squashed baseline:** The repo was switched to a single `0001_qtss_baseline.sql` that **replaces** the old `0001_*.sql` (and the rest of the chain was merged into it). The database may still hold the **old** checksum for version 1 (from the previous `0001` file). That produces the same error for **N = 1**.

### Safe fix when disk matches what actually ran (cosmetic / mistaken edit)

Only if you are sure the database schema matches the SQL currently on disk:

```bash
# From repo root; DATABASE_URL must point at the same database the API/worker uses.
cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums
```

Then restart API/worker.

**Do not** use this to paper over a wrong schema. If the wrong migration was applied, restore the original file or repair the database, then align checksums.

### Duplicate `NNNN_*.sql` prefix

```bash
# Every prefix must appear on exactly one file:
ls migrations/0001*.sql   # expect a single line
```

Rename or remove duplicates so each version number is unique, then rerun migrations (and checksum sync only if appropriate).

### `bar_intervals` and `0036_bar_intervals_repair_if_missing.sql`

Historical repair migration **0036** (and related idempotent `bar_intervals` DDL) is **merged into** `migrations/0001_qtss_baseline.sql` (search for `merged from: 0036_bar_intervals` and `to_regclass('public.bar_intervals')`). There is no separate `0036_*.sql` file in the squashed layout.

If you see errors about **`bar_intervals` missing**, the database is usually **out of step** with the baseline (partial apply, old chain, or manual drops). Fix the schema (restore backup, re-apply from a known-good migration set, or recreate DB — see `migrations/README.md` **Breaking change**), not only checksums.

### Squashed baseline vs database that applied the old multi-version chain

Current layout is documented in `migrations/README.md`:

- **New database:** only `0001_qtss_baseline.sql` applies; one row in `_sqlx_migrations` for version **1**.
- **Old database:** may have rows for versions **1 … N** from the pre-squash file chain. Deploying only `0001_qtss_baseline.sql` on disk causes SQLx to report applied versions **missing from the filesystem** (after any checksum issue is fixed), because versions **2+** exist in `_sqlx_migrations` but not on disk.

**Supported approaches**

1. **New empty database** (or drop/recreate) and migrate + seed as needed — preferred when data loss is acceptable.
2. **Restore the pre-squash `migrations/*.sql` tree** from git (same commit the database was built with) on the server so disk matches `_sqlx_migrations`, then plan a controlled migration/squash separately.

**Advanced (only with ops approval):** If the live schema already matches the **full** squashed baseline (you have verified this), you may align SQLx state by removing `_sqlx_migrations` rows for versions **greater than 1**, then running **`qtss-sync-sqlx-checksums`** so version **1**’s checksum matches `0001_qtss_baseline.sql`. Wrong assumptions here cause SQLx to think migrations are satisfied when they are not.

### Regenerating the squashed file

```bash
python3 scripts/squash_migrations_into_one.py
```

See `scripts/squash_migrations_into_one.py` and `migrations/README.md` for when this is safe and when it is breaking.

### CI and local verification

- `cargo test -p qtss-storage --test migrations_apply` (with `DATABASE_URL`) applies all files under `migrations/` on a fresh schema.

---

*Maintenance: When adding new code references to “§6” in errors or comments, keep this section accurate; point narrative updates to `QTSS_MASTER_DEV_GUIDE.md` §5 where appropriate.*
