-- 0112_wyckoff_phase_progression_fix.sql
--
-- Faz 8.2 — Wyckoff yapılarının Phase C'ye ilerleyememesi ve hiç
-- tamamlanamaması buglarının tanı sonrası migration'ı.
--
-- Diagnostic: 383 structs, 0 completed, 377 failed — TTL guard tarihsel
-- scan'deki `time_ms`'i wall-clock `now()` ile karşılaştırıyordu ve her
-- yapı 2. event'te TTL aşımından ölüyordu. Kod fix'i TTL'yi event→event
-- delta'sına indirdi; ayrıca non-monotonic arrival'lar için chronology
-- guard eklendi.
--
-- Bu migration CLAUDE.md #2 gereği hardcoded kalmış iki sayıyı config'e
-- taşır:
--   * `FAILED_PHASE_C_WINDOW = 12` (structure.rs)
--   * `maybe_inject_markup_markdown`'daki 4 parametre (orchestrator)

-- Failed Phase-C watchdog penceresi.
SELECT _qtss_register_key(
    'failed_phase_c_window_bars','wyckoff','wyckoff','int',
    '12'::jsonb, 'bars',
    'Spring/UTAD sonrası karşı pivot gelirse yapıyı fail işaretleyen bar penceresi.',
    'number', false, 'normal', ARRAY['wyckoff','phase_c']);

-- Markup/Markdown synthetic injection parametreleri.
-- Detector gerçek Markup emit etmiyor; orchestrator Phase D tracker'a
-- son N barın ≥ confirm_pct'i range ± breakout_tol_pct dışında kapandıysa
-- sentetik Markup/Markdown enjekte ediyor → Phase E → completed.
SELECT _qtss_register_key(
    'markup_inject.window_bars','wyckoff','wyckoff','int',
    '30'::jsonb, 'bars',
    'Markup/Markdown injection: son N barlık pencere boyu.',
    'number', false, 'normal', ARRAY['wyckoff','markup_inject']);

SELECT _qtss_register_key(
    'markup_inject.min_window_bars','wyckoff','wyckoff','int',
    '10'::jsonb, 'bars',
    'Markup injection için minimum gerekli bar sayısı (altı → atla).',
    'number', false, 'normal', ARRAY['wyckoff','markup_inject']);

SELECT _qtss_register_key(
    'markup_inject.confirm_pct','wyckoff','wyckoff','float',
    '60.0'::jsonb, 'percent',
    'Markup tetiklemek için pencere içinde breakout threshold'' ini aşan bar yüzdesi.',
    'number', false, 'normal', ARRAY['wyckoff','markup_inject']);

SELECT _qtss_register_key(
    'markup_inject.breakout_tol_pct','wyckoff','wyckoff','float',
    '0.5'::jsonb, 'percent',
    'Range top/bottom etrafındaki breakout tolerans bandı (yüzde).',
    'number', false, 'normal', ARRAY['wyckoff','markup_inject']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('wyckoff','failed_phase_c_window_bars','12'::jsonb,'Spring/UTAD sonrası karşı pivot watchdog window (bars).'),
    ('wyckoff','markup_inject.window_bars','30'::jsonb,'Markup injection window size (bars).'),
    ('wyckoff','markup_inject.min_window_bars','10'::jsonb,'Markup injection minimum window (bars).'),
    ('wyckoff','markup_inject.confirm_pct','60.0'::jsonb,'Markup injection breakout confirm ratio (%).'),
    ('wyckoff','markup_inject.breakout_tol_pct','0.5'::jsonb,'Markup injection breakout tolerance around range (%).')
ON CONFLICT (module, config_key) DO NOTHING;
