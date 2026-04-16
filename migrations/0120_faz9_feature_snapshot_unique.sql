-- 0120_faz9_feature_snapshot_unique.sql
--
-- Faz 9.2.3 — Feature store write fix.
--
-- Original index in 0116 was partial (`WHERE detection_id IS NOT NULL`),
-- so `INSERT ... ON CONFLICT (detection_id, source, feature_spec_version)`
-- could not be matched by Postgres → every DerivativesSource write
-- returned "there is no unique or exclusion constraint matching the
-- ON CONFLICT specification" and the row was silently dropped.
--
-- Rewrite as a regular unique index. Two NULL detection_id rows for the
-- same (source, spec_version) are allowed because PG treats NULLs as
-- distinct; acceptable — detection_id is effectively always bound at
-- write time.

DROP INDEX IF EXISTS uq_features_snap_detection_source;

CREATE UNIQUE INDEX IF NOT EXISTS uq_features_snap_detection_source
    ON qtss_features_snapshot (detection_id, source, feature_spec_version);
