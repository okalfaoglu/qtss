-- FAZ 25.3.A.1 — Major Dip Detection composite scorer.
--
-- User: "elliott itkiyi tespit edecek detaylı kapsamlı ve doğru veri
-- içeren MAJOR_DIP_DETECTION_RESEARCH.md çalışman eksiksiz tamamlandı
-- mı? evet ise adım adım geliştirmeye başla."
--
-- Research doc (docs/MAJOR_DIP_DETECTION_RESEARCH.md §VIII + §XII)
-- specifies an 8-component composite:
--   structural_completion (0.20)  fib_retrace_quality   (0.15)
--   volume_capitulation   (0.15)  cvd_divergence        (0.10)
--   indicator_alignment   (0.10)  sentiment_extreme     (0.10)
--   multi_tf_confluence   (0.10)  funding_oi_signals    (0.10)
-- Each component returns 0..1; the worker writes the composite +
-- per-component breakdown here so the GUI can render the radar chart
-- and IQ-D / IQ-T candidate loops can gate setup creation on the
-- composite score (faz 25.3.B).

CREATE TABLE IF NOT EXISTS major_dip_candidates (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange        TEXT NOT NULL,
    segment         TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    candidate_bar   BIGINT NOT NULL,
    candidate_time  TIMESTAMPTZ NOT NULL,
    candidate_price NUMERIC(38, 18) NOT NULL,
    score           DOUBLE PRECISION NOT NULL,
    components      JSONB NOT NULL,
    verdict         TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS major_dip_candidates_uniq_bar
    ON major_dip_candidates (exchange, segment, symbol, timeframe, candidate_bar);

CREATE INDEX IF NOT EXISTS major_dip_candidates_recent_idx
    ON major_dip_candidates (symbol, timeframe, candidate_time DESC);

CREATE INDEX IF NOT EXISTS major_dip_candidates_score_idx
    ON major_dip_candidates (score DESC, candidate_time DESC);

-- Config seeds — operator can flip weights live without restart.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('major_dip', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the major-dip composite scorer worker.'),
    ('major_dip', 'tick_secs',
     '{"secs": 60}'::jsonb,
     'How often the major dip worker recomputes scores. 60s default.'),
    ('major_dip', 'min_score_for_setup',
     '{"value": 0.55}'::jsonb,
     'IQ-D / IQ-T candidate loops skip setup creation below this composite score.'),
    ('major_dip', 'weights.structural_completion',
     '{"value": 0.20}'::jsonb,
     'Weight for the Elliott structural-completion component.'),
    ('major_dip', 'weights.fib_retrace_quality',
     '{"value": 0.15}'::jsonb,
     'Weight for the Fib retracement zone score component.'),
    ('major_dip', 'weights.volume_capitulation',
     '{"value": 0.15}'::jsonb,
     'Weight for the Wyckoff selling-climax component.'),
    ('major_dip', 'weights.cvd_divergence',
     '{"value": 0.10}'::jsonb,
     'Weight for the CVD bullish-divergence component.'),
    ('major_dip', 'weights.indicator_alignment',
     '{"value": 0.10}'::jsonb,
     'Weight for the RSI/MACD alignment component.'),
    ('major_dip', 'weights.sentiment_extreme',
     '{"value": 0.10}'::jsonb,
     'Weight for the Fear & Greed extreme-fear component.'),
    ('major_dip', 'weights.multi_tf_confluence',
     '{"value": 0.10}'::jsonb,
     'Weight for the parent-TF Elliott alignment component.'),
    ('major_dip', 'weights.funding_oi_signals',
     '{"value": 0.10}'::jsonb,
     'Weight for the funding-rate + OI clean-reset component.')
ON CONFLICT (module, config_key) DO UPDATE
   SET value = EXCLUDED.value,
       description = EXCLUDED.description,
       updated_at = now();
