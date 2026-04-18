-- 0161_trail_mode.sql
--
-- Faz 9.7.5 — AI-advised TP-approach & trailing stop mode.
--
-- Watcher, TP fiyatına yaklaşıldığında (approach_pct) AI'ye danışır. AI
-- TRAIL önerirse pozisyon bu hedefi alıp kapanmak yerine trailing-stop
-- moduna geçer; her yeni ekstremum (long=HH, short=LL) SL'i yukarı/aşağı
-- çeker. AI tekrar sorulmasını önlemek için `ai_advised_tp_idx` tutulur.
--
-- Idempotent: ADD COLUMN IF NOT EXISTS + ON CONFLICT DO NOTHING.

ALTER TABLE qtss_setups
  ADD COLUMN IF NOT EXISTS trail_mode          BOOLEAN     NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS trail_anchor        NUMERIC(18,8),
  ADD COLUMN IF NOT EXISTS ai_advised_tp_idx   SMALLINT,
  ADD COLUMN IF NOT EXISTS ai_advised_at       TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_qtss_setups_trail_mode
  ON qtss_setups (trail_mode)
  WHERE trail_mode = TRUE;

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('notify', 'smart_target.approach_enabled', 'true'::jsonb,
   'AI-advised TP-approach danışmanlık akışını aç/kapat.'),
  ('notify', 'smart_target.tp_approach_pct', '0.003'::jsonb,
   'TP fiyatına bu orana kadar (fraksiyon) yaklaştığında AI''ye sor.'),
  ('notify', 'smart_target.trail_buffer_pct', '0.008'::jsonb,
   'Trail aktifken SL''in anchor''dan geri çekilme payı (fraksiyon).'),
  ('notify', 'smart_target.trail_on_last_tp_only', 'true'::jsonb,
   'Trail yalnızca son TP''de önerilsin (önceki TP''lerde Scale/Ride baskın).')
ON CONFLICT (module, config_key) DO NOTHING;
