-- 0035_qtss_v2_setup_telegram_enable.sql
--
-- Faz 8.0 follow-up — turn on the Setup → Telegram outbox drain.
--
-- Symptom (prod 2026-04-10): qtss_v2_setup_events tablosunda 3 satır
-- delivery_state='pending' ile birikmiş, hiçbir çıkış olmamış. Setup
-- Engine outbox'ı düzgün besliyor, ama qtss-worker içindeki
-- v2_setup_telegram_loop master switch'ini bekliyor:
--   resolve_worker_enabled_flag(pool, "setup.notify.telegram",
--       "enabled", "QTSS_SETUP_NOTIFY_TG_ENABLED", false)
-- Default false → loop sleep döngüsünde takılı kalıyor.
--
-- Bu migration sadece master switch'i true yapar. Bot token + chat_id
-- zaten `notify.telegram_bot_token` / `notify.telegram_chat_id`
-- satırlarında set; apply_notify_telegram_system_config bunları
-- dispatcher'a merge ediyor → ekstra bir şey gerekmiyor.
--
-- Idempotent: zaten varsa overwrite eder; sıfırdan kurulumda insert.

INSERT INTO system_config (module, config_key, value, updated_at)
VALUES ('setup.notify.telegram', 'enabled', 'true'::jsonb, NOW())
ON CONFLICT (module, config_key) DO UPDATE
   SET value = 'true'::jsonb,
       updated_at = NOW();

-- Bonus: birikmiş eski 'failed' satırlar (retries >= MAX_RETRIES) varsa
-- onlara dokunma — loop tarafı zaten skip ediyor. 'pending' satırlar bir
-- sonraki tick'te (≤15s) gönderilecek.
