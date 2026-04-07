-- Intake playbook runs: rule-based / LLM-postprocessed symbol lists for Elliott, ACP, TBM, signals, AI gating.
-- Worker: `intake_playbook_engine`; API: `/api/v1/analysis/intake-playbook/*`.

CREATE TABLE IF NOT EXISTS intake_playbook_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    playbook_id TEXT NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    market_mode TEXT,
    confidence_0_100 INT NOT NULL DEFAULT 0,
    key_reason TEXT,
    neutral_guidance TEXT,
    summary_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    inputs_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    meta_json JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_intake_playbook_runs_playbook_computed ON intake_playbook_runs (playbook_id, computed_at DESC);

CREATE TABLE IF NOT EXISTS intake_playbook_candidates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    run_id UUID NOT NULL REFERENCES intake_playbook_runs (id) ON DELETE CASCADE,
    rank INT NOT NULL,
    symbol TEXT NOT NULL,
    chain TEXT,
    direction TEXT NOT NULL,
    intake_tier TEXT NOT NULL DEFAULT 'scan',
    confidence_0_100 INT NOT NULL DEFAULT 0,
    detail_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    merged_engine_symbol_id UUID REFERENCES engine_symbols (id) ON DELETE SET NULL,
    merged_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_intake_playbook_candidates_run ON intake_playbook_candidates (run_id, rank ASC);

COMMENT ON TABLE intake_playbook_runs IS 'Smart-money playbook sweep output (market_mode, elite_*, ten_x, scanners).';
COMMENT ON TABLE intake_playbook_candidates IS 'Ranked symbols per run; optional promote to engine_symbols via merged_* columns.';

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
    (
        'worker',
        'intake_playbook_loop_enabled',
        '{"enabled":false}'::jsonb,
        'intake_playbook_engine: persist market_mode + scanner candidates to intake_playbook_*',
        false
    ),
    (
        'worker',
        'intake_playbook_tick_secs',
        '{"secs":300}'::jsonb,
        'Seconds between intake playbook sweeps',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
