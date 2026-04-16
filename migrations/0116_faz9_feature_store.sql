-- 0116_faz9_feature_store.sql
--
-- Faz 9.0.2 — Feature Store.
--
-- Her detection anında `ConfluenceSource` implementasyonları bir
-- feature snapshot yazar. AI meta-model eğitiminde `qtss_setup_outcomes`
-- ile `(setup_id)` üzerinden join edilir.
--
-- Neden iki sütun (`features_json` + `feature_spec_version`)?
--   * JSONB sürüm A → B feature ekleme geriye dönük compatible
--   * `feature_spec_version` sayısı model training pipeline'ında filtreyi
--     garantiler (eski spec'li satırları eğitime almadan ayırır).

CREATE TABLE IF NOT EXISTS qtss_features_snapshot (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    detection_id         UUID REFERENCES qtss_v2_detections(id) ON DELETE CASCADE,
    setup_id             UUID REFERENCES qtss_v2_setups(id) ON DELETE SET NULL,
    exchange             TEXT NOT NULL,
    symbol               TEXT NOT NULL,
    timeframe            TEXT NOT NULL,
    source               TEXT NOT NULL,       -- 'wyckoff','elliott','derivatives',...
    feature_spec_version INTEGER NOT NULL,
    features_json        JSONB NOT NULL,
    computed_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    computed_at_bar_ms   BIGINT,              -- event bar time_ms (training bias avoidance)
    meta_json            JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT features_snapshot_src_chk CHECK (source <> '')
);

CREATE INDEX IF NOT EXISTS idx_features_snap_detection ON qtss_features_snapshot (detection_id);
CREATE INDEX IF NOT EXISTS idx_features_snap_setup     ON qtss_features_snapshot (setup_id);
CREATE INDEX IF NOT EXISTS idx_features_snap_symbol    ON qtss_features_snapshot (exchange, symbol, timeframe, computed_at DESC);
CREATE INDEX IF NOT EXISTS idx_features_snap_source    ON qtss_features_snapshot (source, feature_spec_version);
-- Aynı (detection_id, source, spec_version) için idempotent yazım.
CREATE UNIQUE INDEX IF NOT EXISTS uq_features_snap_detection_source
    ON qtss_features_snapshot (detection_id, source, feature_spec_version)
    WHERE detection_id IS NOT NULL;

COMMENT ON TABLE qtss_features_snapshot IS 'Faz 9.0.2 — per-detection feature vector per ConfluenceSource; join setup_id → qtss_setup_outcomes for labels.';

-- Feature store config (CLAUDE.md #2).
SELECT _qtss_register_key(
    'feature_store.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Master switch: detection hook writes feature snapshots.',
    'bool', false, 'normal', ARRAY['ai','feature_store']);

SELECT _qtss_register_key(
    'feature_store.spec_version','ai','feature_store','int',
    '1'::jsonb, '',
    'Current feature spec version (bump on breaking schema change).',
    'number', false, 'normal', ARRAY['ai','feature_store']);

SELECT _qtss_register_key(
    'feature_store.sources.wyckoff.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Wyckoff ConfluenceSource feature extraction.',
    'bool', false, 'normal', ARRAY['ai','feature_store','wyckoff']);

SELECT _qtss_register_key(
    'feature_store.sources.derivatives.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Derivatives (OI/funding/LS/liq/CVD) ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','derivatives']);

SELECT _qtss_register_key(
    'feature_store.sources.regime.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Regime Deep ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','regime']);

SELECT _qtss_register_key(
    'feature_store.sources.tbm.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable TBM ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','tbm']);

SELECT _qtss_register_key(
    'feature_store.sources.classical.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Classical pattern ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','classical']);

SELECT _qtss_register_key(
    'feature_store.sources.elliott.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Elliott wave ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','elliott']);

SELECT _qtss_register_key(
    'feature_store.sources.session.enabled','ai','feature_store','bool',
    'true'::jsonb, '',
    'Enable Session/volatility ConfluenceSource.',
    'bool', false, 'normal', ARRAY['ai','feature_store','session']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('ai','feature_store.enabled','true'::jsonb,'Feature store master switch.'),
    ('ai','feature_store.spec_version','1'::jsonb,'Feature spec v1 (initial).'),
    ('ai','feature_store.sources.wyckoff.enabled','true'::jsonb,'Wyckoff source.'),
    ('ai','feature_store.sources.derivatives.enabled','true'::jsonb,'Derivatives source.'),
    ('ai','feature_store.sources.regime.enabled','true'::jsonb,'Regime source.'),
    ('ai','feature_store.sources.tbm.enabled','true'::jsonb,'TBM source.'),
    ('ai','feature_store.sources.classical.enabled','true'::jsonb,'Classical source.'),
    ('ai','feature_store.sources.elliott.enabled','true'::jsonb,'Elliott source.'),
    ('ai','feature_store.sources.session.enabled','true'::jsonb,'Session source.')
ON CONFLICT (module, config_key) DO NOTHING;
