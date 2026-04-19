-- 0169_faz_9b_training_infra.sql
--
-- Faz 9B — AI eğitim/inference altyapısını canlıya almak için eksik
-- konfigürasyon anahtarları + otomasyon kontrol tabloları. Her parça
-- CLAUDE.md #2 uyumlu (eşikler DB'de, kodda değil).
--
-- Bölümler:
--   A) trainer konfigürasyonu (min_rows, coverage_gate, lgbm_params)
--   B) inference sidecar konfigürasyonu (bind, timeout, shap_cache)
--   C) retraining automation konfigürasyonu (cron interval, triggers)
--   D) drift circuit breaker eşikleri
--   E) historical backfill konfigürasyonu
--   F) qtss_ml_training_runs — her training job için audit log
--
-- Idempotent.

-- ---------------------------------------------------------------------
-- A) TRAINER
-- ---------------------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ai', 'trainer.enabled', 'true'::jsonb,
   'Master switch: cron-scheduled retraining loop.'),
  ('ai', 'trainer.min_rows', '500'::jsonb,
   'Minimum closed+labeled setups required to trigger training.'),
  ('ai', 'trainer.min_feature_coverage_pct', '0.60'::jsonb,
   'Her feature için min dolu-oran (%). Altında kalan feature eğitim matrisinden düşer; %60 altında coverage ise eğitim hiç çalışmaz.'),
  ('ai', 'trainer.min_label_balance', '0.15'::jsonb,
   'Minority-class oranı bu eşiğin altındaysa eğitim red (class imbalance guard).'),
  ('ai', 'trainer.validation_fraction', '0.2'::jsonb,
   'Time-ordered split: son %N validation, öncesi train.'),
  ('ai', 'trainer.num_boost_round', '500'::jsonb,
   'LightGBM max boosting round (early stopping kesebilir).'),
  ('ai', 'trainer.early_stopping_rounds', '50'::jsonb,
   'Validation metric bu round boyunca iyileşmiyorsa durdur.'),
  ('ai', 'trainer.artifact_dir', '"/app/qtss/artifacts/models"'::jsonb,
   'Model .txt ve .meta.json dosyalarının diskteki kök dizini.'),
  ('ai', 'trainer.model_family', '"setup_meta"'::jsonb,
   'Varsayılan model ailesi (setup_meta | signal_rank | position_size).'),
  ('ai', 'trainer.auto_activate_if_better', 'true'::jsonb,
   'Yeni model mevcut aktiften daha yüksek AUC/PR-AUC çıkarırsa otomatik active=true; aksi halde shadow.'),
  ('ai', 'trainer.auto_activate_min_auc_lift', '0.01'::jsonb,
   'auto_activate için gereken min AUC üstünlüğü (absolute).')
ON CONFLICT (module, config_key) DO NOTHING;

-- ---------------------------------------------------------------------
-- B) INFERENCE SIDECAR
-- ---------------------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ai', 'inference.enabled', 'true'::jsonb,
   'Rust worker → Python sidecar /score çağrısı açık mı?'),
  ('ai', 'inference.bind_host', '"127.0.0.1"'::jsonb,
   'Sidecar uvicorn bind host.'),
  ('ai', 'inference.bind_port', '8790'::jsonb,
   'Sidecar uvicorn bind port; Rust tarafı aynı değeri okur.'),
  ('ai', 'inference.http_timeout_ms', '2000'::jsonb,
   'Rust → sidecar HTTP timeout. Setup engine inline çağırıyor, sert tutulmalı.'),
  ('ai', 'inference.fail_open', 'false'::jsonb,
   'Sidecar 503 veya timeout verirse: true → setup AI skoru olmadan geçer, false → setup reddedilir (safe default).'),
  ('ai', 'inference.shap_cache_size', '1024'::jsonb,
   'LRU: feature-vector hash → SHAP explanation. 0 = cache kapalı.'),
  ('ai', 'inference.shap_on_every_call', 'false'::jsonb,
   'false → SHAP sadece /explain çağrısında; true → her /score cevabıyla SHAP ek (latency ağır).')
ON CONFLICT (module, config_key) DO NOTHING;

-- ---------------------------------------------------------------------
-- C) RETRAINING AUTOMATION
-- ---------------------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ai', 'retraining.cron_interval_secs', '3600'::jsonb,
   'Scheduler her N saniyede bir "eğitim gerekli mi" kontrolü yapar.'),
  ('ai', 'retraining.trigger_min_new_closed', '50'::jsonb,
   'Aktif modelin trained_at''ından beri bu kadar yeni closed setup birikmişse retrain tetikle.'),
  ('ai', 'retraining.trigger_age_hours', '168'::jsonb,
   'Aktif model bu saatten eskiyse (default 7 gün), yeni veri olmasa bile retrain tetikle.'),
  ('ai', 'retraining.max_concurrent', '1'::jsonb,
   'Paralel training job limiti. >1 resource savaşı yaratır, 1 tutulmalı.'),
  ('ai', 'retraining.binary_path', '"qtss-trainer"'::jsonb,
   'Scheduler''ın çalıştıracağı trainer binary (PATH''de veya absolute).')
ON CONFLICT (module, config_key) DO NOTHING;

-- ---------------------------------------------------------------------
-- D) DRIFT CIRCUIT BREAKER
-- ---------------------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ai', 'drift.psi_warning_threshold', '0.10'::jsonb,
   'PSI: 0.10-0.25 arası warning; log + GUI banner, model hâlâ active.'),
  ('ai', 'drift.psi_critical_threshold', '0.25'::jsonb,
   'PSI ≥ 0.25: feature distribution dramatik değişmiş. Breaker açılır.'),
  ('ai', 'drift.critical_features_for_trip', '3'::jsonb,
   'Breaker tripping için en az N feature''ın critical olması gerekir (gürültü filtresi).'),
  ('ai', 'drift.breaker_action', '"deactivate"'::jsonb,
   'Breaker aksiyonu: deactivate (active=false), alert_only (log+outbox), throttle (setup üretimi yavaşlat).'),
  ('ai', 'drift.check_interval_secs', '1800'::jsonb,
   'PSI hesaplama scheduler periyodu (30 dk).'),
  ('ai', 'drift.psi_lookback_hours', '24'::jsonb,
   'Live distribution penceresi: son N saatlik prediction feature snapshot''ları.')
ON CONFLICT (module, config_key) DO NOTHING;

-- ---------------------------------------------------------------------
-- E) HISTORICAL BACKFILL
-- ---------------------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ai', 'backfill.enabled', 'false'::jsonb,
   'Tarihsel replay ile training set üretimi (Faz 9B walk-forward). Master switch default kapalı; operator elle açar.'),
  ('ai', 'backfill.warmup_bars', '500'::jsonb,
   'Detector replay için gereken minimum önceki bar sayısı (pivot tree, ATR, SMA200).'),
  ('ai', 'backfill.lookhead_bars', '240'::jsonb,
   'Setup forward-simulate penceresi (kaç bar ileriye bak, TP/SL resolution için).'),
  ('ai', 'backfill.timeframes', '["H1","H4","D1"]'::jsonb,
   'Replay edilecek timeframe listesi. M5/M15 büyük veri yaratır, default dışı.'),
  ('ai', 'backfill.symbols_limit', '100'::jsonb,
   'Tek turda replay edilecek max sembol sayısı (resource guard).'),
  ('ai', 'backfill.setup_mode_marker', '"backtest"'::jsonb,
   'Backfill setuplar qtss_setups.mode=''backtest'' ile yazılır. Live verilerden ayırmak için.')
ON CONFLICT (module, config_key) DO NOTHING;

-- ---------------------------------------------------------------------
-- F) TRAINING RUNS AUDIT LOG
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS qtss_ml_training_runs (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  started_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  finished_at    TIMESTAMPTZ,
  trigger_source TEXT NOT NULL CHECK (trigger_source IN (
                   'manual','cron','drift','backfill','outcome_milestone')),
  status         TEXT NOT NULL DEFAULT 'running' CHECK (status IN (
                   'running','success','failed','skipped_insufficient_data',
                   'skipped_low_coverage','skipped_imbalance')),
  n_closed_setups INTEGER,
  n_features     INTEGER,
  feature_coverage_pct REAL,
  label_balance  REAL,
  model_id       UUID REFERENCES qtss_models(id),
  error_msg      TEXT,
  notes          TEXT
);
CREATE INDEX IF NOT EXISTS idx_training_runs_started  ON qtss_ml_training_runs(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_training_runs_status   ON qtss_ml_training_runs(status);
CREATE INDEX IF NOT EXISTS idx_training_runs_trigger  ON qtss_ml_training_runs(trigger_source);

COMMENT ON TABLE qtss_ml_training_runs IS
  'Faz 9B — her training tetikleyicisi (cron/manual/drift/backfill) için audit satırı. GUI ve runbook''lar buradan okur.';

-- ---------------------------------------------------------------------
-- G) DRIFT CIRCUIT BREAKER STATE
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS qtss_ml_breaker_events (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  fired_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  model_id       UUID NOT NULL REFERENCES qtss_models(id),
  action         TEXT NOT NULL,          -- deactivate | alert_only | throttle
  reason         TEXT NOT NULL,          -- "3 features PSI > 0.25"
  critical_features JSONB NOT NULL,      -- [{feature, psi}, ...]
  resolved_at    TIMESTAMPTZ,
  resolved_by    TEXT,
  resolution_note TEXT
);
CREATE INDEX IF NOT EXISTS idx_breaker_events_fired ON qtss_ml_breaker_events(fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_breaker_events_open  ON qtss_ml_breaker_events(resolved_at) WHERE resolved_at IS NULL;

COMMENT ON TABLE qtss_ml_breaker_events IS
  'Faz 9B — PSI drift circuit breaker tetiklendiğinde bir satır. Operator resolved_at doldurarak kapatır (runbook step 7).';
