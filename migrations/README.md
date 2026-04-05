# SQL migrations (`migrations/*.sql`)

Applied at API/worker startup via `qtss_storage::run_migrations` (SQLx).

## Current layout (squashed baseline)

Tek dosya:

- **`0001_qtss_baseline.sql`** — tüm şema + tohumlar + sonradan eklenen parçalar (`-- >>> merged from:` / `-- >>> squashed from:` bölüm başlıklarıyla). Eski zincir `0001`…`0063` ve son delta parçaları burada birleşiktir; dizinde yalnız bu dosya kalmalıdır.

Yeni şema değişikliği: geçici olarak `0002_yeni.sql` ekleyip `python3 scripts/squash_migrations_into_one.py` ile tekrar tek dosyaya indir (veya üretimde zaten uygulanmış baseline’ı elleme — yeni numaralı dosya + squash / dokümantasyondaki reset kuralları).

**Regenerate** (split dosyalar varken veya git’ten eski `migrations/*.sql` seti geri alındıktan sonra):

```bash
# From repo root; requires Python 3
python3 scripts/squash_migrations_into_one.py
# Windows (if python3 not on PATH):
py -3 scripts/squash_migrations_into_one.py
```

The script expects at least one `NNNN_*.sql` input **other than** `0001_qtss_baseline.sql`. Yalnız `0001_qtss_baseline.sql` varsa talimat yazıp çıkar (nothing to do).

To re-squash from git history, check out the old `migrations/*.sql` set (move the current baseline aside if it blocks checkout), run the script, then commit.

## Breaking change

Databases that already applied the **old** multi-file chain (`_sqlx_migrations` with versions 2–63) are **not** compatible with this single `0001` baseline (checksum and version sequence differ).

**New environments:** empty database → run API/worker → migration applies once.

**Existing environments:** either keep the pre-squash migration files (restore from git before squash commit) or **drop and recreate** the database and re-seed as needed.

## Rules (summary)

- Normal durumda dizinde **tek** `0001_qtss_baseline.sql` olur. Geçici çalışmada birden fazla `NNNN_*.sql` varken aynı numara iki dosyada kullanılamaz.
- Üretimde uygulanmış baseline satırını değiştirme; yeni delta için yeni numaralı dosya + squash veya yeni boş DB ile baseline yenileme.
- If your workflow uses offline SQLx query data, refresh it after schema changes (`qtss-sync-sqlx-checksums` / project conventions).
