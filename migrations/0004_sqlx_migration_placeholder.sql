-- Placeholder migration for environments where `_sqlx_migrations` version 4 was recorded
-- but the original `0004_*.sql` file is not in this tree (branch / checkout drift).
--
-- If `cargo run -p qtss-api` fails with **checksum mismatch** on version 4 instead of "missing",
-- your DB applied different SQL than this file. Then either restore the original migration file
-- from backup, or (dev only, after confirming schema) remove the stale row and re-apply:
--   DELETE FROM _sqlx_migrations WHERE version = 4;
-- Then restart the API/worker once so this placeholder runs and re-registers v4.
SELECT 1;
