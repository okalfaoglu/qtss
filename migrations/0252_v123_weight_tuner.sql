-- Weight tuner v1.2.3 — post-trade learning loop.
--
-- ChatGPT/Gemini teardown alpha #5: every closed setup leaves a record
-- in qtss_setups (primary_family, realized_pnl_pct, close_reason).
-- Aggregating those by family gives us a winrate / EV per family;
-- the allocator's confluence weights should pick up the slack
-- automatically — losing families nudged DOWN, winning families
-- nudged UP. Drift cap + sample-size floor keep an outlier week
-- from killing a strategy.
--
-- Tick: hourly. Lookback: 14 days by default.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('weight_tuner', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the post-trade weight tuner loop. When false the loop sleeps an hour and skips computation.'),
    ('weight_tuner', 'tick_secs',
     '{"secs": 3600}'::jsonb,
     'Tick cadence in seconds. Hourly is plenty — closed-setup distribution does not move minute-by-minute.'),
    ('weight_tuner', 'lookback_days',
     '14'::jsonb,
     'Window of closed setups (days) used to compute family EV. 14 = balance between recency and sample size.'),
    ('weight_tuner', 'min_sample',
     '10'::jsonb,
     'Minimum closed setups per family before any nudge fires. Below this cold-start has no signal — leave weight unchanged.'),
    ('weight_tuner', 'max_drift_pct',
     '0.10'::jsonb,
     'Max fractional change to a family weight per tick (0.10 = ±10%). Prevents an outlier week from collapsing a strategy.'),
    ('weight_tuner', 'weight_floor',
     '0.10'::jsonb,
     'Hard floor on absolute family weight — never zero a family out (keep some optionality for it to come back).'),
    ('weight_tuner', 'weight_ceiling',
     '2.00'::jsonb,
     'Hard ceiling on absolute family weight — keep one hot family from trampling the rest.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Audit ledger so we can see exactly when and why a weight moved.
CREATE TABLE IF NOT EXISTS weight_tuner_history (
    id            bigserial PRIMARY KEY,
    fired_at      timestamptz NOT NULL DEFAULT now(),
    family        text        NOT NULL,
    samples       bigint      NOT NULL,
    winrate       double precision NOT NULL,
    avg_win_pct   double precision NOT NULL,
    avg_loss_pct  double precision NOT NULL,
    ev_pct        double precision NOT NULL,
    prev_weight   double precision NOT NULL,
    new_weight    double precision NOT NULL
);

CREATE INDEX IF NOT EXISTS weight_tuner_history_family_idx
    ON weight_tuner_history (family, fired_at DESC);
