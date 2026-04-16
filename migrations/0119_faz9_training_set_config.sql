-- 0119_faz9_training_set_config.sql
--
-- Faz 9.2.1 — readiness thresholds for the Training Set Monitor.
-- Default: 500 closed setups with ≥80% feature coverage before the
-- Faz 9.3 LightGBM trainer is considered trainable. Operators override
-- via Config Editor, no redeploy.

SELECT _qtss_register_key(
    'training_set.min_closed',
    'setup',
    'training_set',
    'int',
    '500'::jsonb,
    '',
    'Minimum number of closed+labeled setups before the trainer spins up.',
    'count',
    false,
    'normal',
    ARRAY['setup','training_set','faz9']
);

SELECT _qtss_register_key(
    'training_set.min_feature_coverage_pct',
    'setup',
    'training_set',
    'float',
    '80.0'::jsonb,
    '',
    'Minimum %% of setups that must carry at least one feature snapshot row.',
    'percent',
    false,
    'normal',
    ARRAY['setup','training_set','faz9']
);
