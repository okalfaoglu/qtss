-- Faz 11: Regime Deep — config seed

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('regime', 'multi_tf_enabled',             'true',  'Multi-timeframe regime confluence'),
  ('regime', 'tf_weights',                   '{"5m":0.1,"15m":0.15,"1h":0.25,"4h":0.30,"1d":0.20}', 'TF confluence weights'),
  ('regime', 'timeframes',                   '["15m","1h","4h","1d"]', 'Timeframes for regime computation'),
  ('regime', 'transition_detection_enabled', 'true',  'Detect regime transitions'),
  ('regime', 'transition_min_confidence',    '0.6',   'Min confidence for transition alert'),
  ('regime', 'hmm_enabled',                  'false', 'HMM regime prediction (config-gated)'),
  ('regime', 'hmm_train_bars',               '1000',  'Bars to train HMM'),
  ('regime', 'hmm_retrain_interval_h',       '24',    'Retrain interval in hours'),
  ('regime', 'adaptive_params_enabled',      'true',  'Auto-adjust params per regime'),
  ('regime', 'snapshot_tick_secs',           '300',   'Regime snapshot loop interval'),
  ('regime', 'snapshot_retention_days',      '7',     'Days to keep snapshots')
ON CONFLICT (module, config_key) DO NOTHING;
