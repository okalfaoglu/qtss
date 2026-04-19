-- 0170_faz_9b_worker_toggles.sql
--
-- Faz 9B — worker task'ların (psi_drift_loop + ml_backfill_orchestrator)
-- enable/polling anahtarları. 0169 migration'ı PSI eşiklerini seed etmişti
-- ama çalıştıran worker'ın master switch'i + backfill orchestrator'ın poll
-- parametreleri eksikti. Bu migration onları ekler.
--
-- Idempotent.

INSERT INTO system_config (module, config_key, value, description) VALUES
  -- PSI drift circuit breaker master switch (psi_drift_loop.rs)
  ('ai', 'drift.enabled', 'true'::jsonb,
   'PSI drift circuit breaker worker master switch. false → worker tick atmaz, breaker asla tripmaz.'),

  -- Historical backfill orchestrator poll params (ml_backfill_orchestrator.rs)
  ('ai', 'backfill.poll_secs', '120'::jsonb,
   'Orchestrator backtest-mode setup count''ını bu aralıkla kontrol eder.'),
  ('ai', 'backfill.plateau_secs', '600'::jsonb,
   'Yeni backtest setup üretilmezse bu saniye sonra döngü biter (default 10 dk).')
ON CONFLICT (module, config_key) DO NOTHING;
