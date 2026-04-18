-- 0159_rename_setups_positions.sql
--
-- Tablo rename: qtss_v2_setups → qtss_setups, q_radar_positions → qtss_positions.
-- FK referansları PostgreSQL tarafından otomatik güncellenir (alan tabloları
-- yeni isme bağlı kalır). Index/constraint isimlerini de eş tutmak için
-- birlikte RENAME edilir. Idempotent: IF EXISTS ile çift çalıştırmaya güvenli.

DO $$
BEGIN
  -- ---------------------------------------------------------------------
  -- qtss_v2_setups → qtss_setups
  -- ---------------------------------------------------------------------
  IF EXISTS (SELECT 1 FROM pg_tables WHERE tablename='qtss_v2_setups') THEN
    ALTER TABLE qtss_v2_setups RENAME TO qtss_setups;
  END IF;

  -- Index rename (idx_v2_setups_* + idx_qtss_v2_setups_* + ux_* + uq_*)
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='qtss_v2_setups_pkey') THEN
    ALTER INDEX qtss_v2_setups_pkey RENAME TO qtss_setups_pkey;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_detection') THEN
    ALTER INDEX idx_v2_setups_detection RENAME TO idx_qtss_setups_detection;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_open') THEN
    ALTER INDEX idx_v2_setups_open RENAME TO idx_qtss_setups_open;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_symbol') THEN
    ALTER INDEX idx_v2_setups_symbol RENAME TO idx_qtss_setups_symbol;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_mode_state') THEN
    ALTER INDEX idx_v2_setups_mode_state RENAME TO idx_qtss_setups_mode_state;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='ux_v2_setups_idempotency_key') THEN
    ALTER INDEX ux_v2_setups_idempotency_key RENAME TO ux_qtss_setups_idempotency_key;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_tracker_cursor') THEN
    ALTER INDEX idx_v2_setups_tracker_cursor RENAME TO idx_qtss_setups_tracker_cursor;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_v2_setups_ai_score') THEN
    ALTER INDEX idx_v2_setups_ai_score RENAME TO idx_qtss_setups_ai_score;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_qtss_v2_setups_active_watcher') THEN
    ALTER INDEX idx_qtss_v2_setups_active_watcher RENAME TO idx_qtss_setups_active_watcher;
  END IF;
  -- uq_open_setup_key is neutral (no version in name) — leave it.

  -- Constraint rename
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_alt_type_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_alt_type_check TO qtss_setups_alt_type_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_close_reason_chk') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_close_reason_chk TO qtss_setups_close_reason_chk;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_confluence_id_fkey') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_confluence_id_fkey TO qtss_setups_confluence_id_fkey;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_detection_id_fkey') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_detection_id_fkey TO qtss_setups_detection_id_fkey;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_direction_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_direction_check TO qtss_setups_direction_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_mode_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_mode_check TO qtss_setups_mode_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_profile_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_profile_check TO qtss_setups_profile_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_state_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_state_check TO qtss_setups_state_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='qtss_v2_setups_venue_class_check') THEN
    ALTER TABLE qtss_setups RENAME CONSTRAINT qtss_v2_setups_venue_class_check TO qtss_setups_venue_class_check;
  END IF;

  -- ---------------------------------------------------------------------
  -- q_radar_positions → qtss_positions
  -- ---------------------------------------------------------------------
  IF EXISTS (SELECT 1 FROM pg_tables WHERE tablename='q_radar_positions') THEN
    ALTER TABLE q_radar_positions RENAME TO qtss_positions;
  END IF;

  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='q_radar_positions_pkey') THEN
    ALTER INDEX q_radar_positions_pkey RENAME TO qtss_positions_pkey;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='q_radar_positions_setup_id_key') THEN
    ALTER INDEX q_radar_positions_setup_id_key RENAME TO qtss_positions_setup_id_key;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_q_radar_positions_setup') THEN
    ALTER INDEX idx_q_radar_positions_setup RENAME TO idx_qtss_positions_setup;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_indexes WHERE indexname='idx_q_radar_positions_open') THEN
    ALTER INDEX idx_q_radar_positions_open RENAME TO idx_qtss_positions_open;
  END IF;

  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='q_radar_positions_direction_check') THEN
    ALTER TABLE qtss_positions RENAME CONSTRAINT q_radar_positions_direction_check TO qtss_positions_direction_check;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='q_radar_positions_setup_id_fkey') THEN
    ALTER TABLE qtss_positions RENAME CONSTRAINT q_radar_positions_setup_id_fkey TO qtss_positions_setup_id_fkey;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='q_radar_positions_state_check') THEN
    ALTER TABLE qtss_positions RENAME CONSTRAINT q_radar_positions_state_check TO qtss_positions_state_check;
  END IF;

  -- ---------------------------------------------------------------------
  -- Geriye kalan gelen-FK'ler (diğer tablolardan referans) — Postgres
  -- otomatik rename etmez; yalnızca isim hijyeni için gerekli olanları
  -- güncelleriz.
  -- ---------------------------------------------------------------------
END$$;

-- Geriye-dönük uyumluluk view'u (eski ismi referans eden dış araçlar için).
-- Kod tabanı rename sonrası sadece qtss_setups kullanır; bu view yalnızca
-- acil rollback/inceleme kolaylığı sağlar. İleride kaldırılabilir.
DROP VIEW IF EXISTS qtss_v2_setups;
CREATE VIEW qtss_v2_setups AS SELECT * FROM qtss_setups;

DROP VIEW IF EXISTS q_radar_positions;
CREATE VIEW q_radar_positions AS SELECT * FROM qtss_positions;
