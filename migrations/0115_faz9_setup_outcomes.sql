-- 0115_faz9_setup_outcomes.sql
--
-- Faz 9.0.1 — Outcome Labeler tablosu.
--
-- Amaç: AI meta-model eğitimi için kapalı setup'ları **tek satır**,
-- değişmez etiketle işaretle. `qtss_v2_setups` satırı güncellenmeye
-- devam eder (retrack vs.), ama outcome tablosuna yazım idempotenttir —
-- `(setup_id)` primary key.
--
-- Label taxonomy (CLAUDE.md #1: look-up/dispatch; if/else yerine):
--   * win           — target_hit veya pnl_pct > 0 & closed_win
--   * loss          — stop_hit, pnl_pct < 0, veya closed_loss
--   * neutral       — closed/closed_manual & |pnl_pct| < threshold
--   * invalidated   — p14_opposite_dir_conflict, reverse_signal (setup
--                     logic tarafından iptal — eğitim için ayrı sınıf)
--   * timeout       — ileri fazda eklenecek (TTL dolması)
--
-- Realized RR = pnl_pct / risk_pct (eğer risk_pct>0). Negatifse loss,
-- pozitifse win magnitude. time_to_outcome_bars, labeler dolduramazsa
-- NULL; `bars_to_close` mevcutsa oradan çekilir.

CREATE TABLE IF NOT EXISTS qtss_setup_outcomes (
    setup_id             UUID PRIMARY KEY REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    label                TEXT NOT NULL CHECK (label IN ('win','loss','neutral','invalidated','timeout')),
    close_reason         TEXT,
    close_reason_category TEXT NOT NULL CHECK (close_reason_category IN ('tp','sl','manual','reverse','conflict','expiry','unknown')),
    realized_rr          REAL,              -- pnl_pct / risk_pct
    pnl_pct              REAL,
    max_favorable_r      REAL,
    max_adverse_r        REAL,
    time_to_outcome_bars INTEGER,
    bars_to_first_tp     INTEGER,
    closed_at            TIMESTAMPTZ,
    labeled_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    labeler_version      INTEGER NOT NULL DEFAULT 1,
    meta_json            JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_setup_outcomes_label       ON qtss_setup_outcomes (label);
CREATE INDEX IF NOT EXISTS idx_setup_outcomes_closed_at   ON qtss_setup_outcomes (closed_at DESC);
CREATE INDEX IF NOT EXISTS idx_setup_outcomes_labeled_at  ON qtss_setup_outcomes (labeled_at DESC);

COMMENT ON TABLE qtss_setup_outcomes IS 'Faz 9.0.1 immutable outcome labels for AI meta-model training.';

-- Worker config (CLAUDE.md #2).
SELECT _qtss_register_key(
    'outcome.labeler.enabled','ai','outcome_labeler','bool',
    'true'::jsonb, '',
    'Periodic sweep of closed qtss_v2_setups → qtss_setup_outcomes label.',
    'bool', false, 'normal', ARRAY['ai','outcome','labeler']);

SELECT _qtss_register_key(
    'outcome.labeler.tick_secs','ai','outcome_labeler','int',
    '120'::jsonb, 'seconds',
    'Outcome labeler sweep cadence.',
    'number', false, 'normal', ARRAY['ai','outcome','labeler']);

SELECT _qtss_register_key(
    'outcome.labeler.neutral_abs_pnl_pct','ai','outcome_labeler','float',
    '0.05'::jsonb, 'percent',
    'abs(pnl_pct) below this → label=neutral (breakeven close).',
    'number', false, 'normal', ARRAY['ai','outcome','labeler']);

SELECT _qtss_register_key(
    'outcome.labeler.backfill_batch','ai','outcome_labeler','int',
    '500'::jsonb, 'rows',
    'Max rows upserted per sweep (throttling).',
    'number', false, 'normal', ARRAY['ai','outcome','labeler']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('ai','outcome.labeler.enabled','true'::jsonb,'Outcome labeler master switch.'),
    ('ai','outcome.labeler.tick_secs','120'::jsonb,'Outcome labeler tick (secs).'),
    ('ai','outcome.labeler.neutral_abs_pnl_pct','0.05'::jsonb,'Neutral band |pnl%|.'),
    ('ai','outcome.labeler.backfill_batch','500'::jsonb,'Max rows per sweep.')
ON CONFLICT (module, config_key) DO NOTHING;
