-- Run against PROD DB to audit migration state.
-- Usage: psql "$PROD_DATABASE_URL" -f deploy/migration_status.sql
-- Side-effect free: SELECT-only.

\echo '=== Applied migrations (_sqlx_migrations) ==='
SELECT version,
       description,
       to_char(installed_on, 'YYYY-MM-DD HH24:MI') AS installed,
       success,
       execution_time
  FROM _sqlx_migrations
 ORDER BY version DESC
 LIMIT 20;

\echo
\echo '=== Rows that did NOT succeed (should be empty) ==='
SELECT version, description, execution_time
  FROM _sqlx_migrations
 WHERE success = false;

\echo
\echo '=== system_config audit: TBM module keys (post-P22..P26) ==='
SELECT config_key,
       value,
       updated_at
  FROM system_config
 WHERE module = 'tbm'
 ORDER BY config_key;

\echo
\echo '=== Detection state distribution (last 7 days) ==='
SELECT state,
       COUNT(*) AS n,
       COUNT(*) FILTER (WHERE family = 'tbm') AS tbm_n
  FROM qtss_v2_detections
 WHERE detected_at > now() - interval '7 days'
 GROUP BY state
 ORDER BY n DESC;

\echo
\echo '=== Hot-path index presence (fast-query preflight) ==='
SELECT indexname
  FROM pg_indexes
 WHERE tablename = 'qtss_v2_detections'
 ORDER BY indexname;

\echo
\echo '=== Table bloat / size snapshot ==='
SELECT relname AS table,
       pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
       pg_size_pretty(pg_relation_size(relid)) AS heap_size,
       n_live_tup,
       n_dead_tup,
       last_autovacuum,
       last_autoanalyze
  FROM pg_stat_user_tables
 WHERE relname IN ('qtss_v2_detections', 'system_config', 'market_bars')
 ORDER BY pg_total_relation_size(relid) DESC;
