-- 0188_position_guard_time_stop_and_hard_invalidation.sql
--
-- Faz D — Time Stop: setup hedefe ulaşmadan vakit doldu → kapat.
-- Faz E — Hard Invalidation: fiyat, detection'ın ürettiği D-point /
--         entry_sl seviyesini aştı → formasyon yapısal olarak öldü,
--         `invalidated` statüsünde ayrı close_reason ile loglanır
--         (koruma ratchet'i BE'ye çekilmiş olsa bile).

-- (a) close_reason CHECK'e 'time_stop' + 'hard_invalidation' ekle -----
DO $$
DECLARE
    cname TEXT;
BEGIN
    FOR cname IN
        SELECT conname
          FROM pg_constraint
         WHERE conrelid = 'qtss_setups'::regclass
           AND contype  = 'c'
           AND pg_get_constraintdef(oid) ILIKE '%close_reason%'
    LOOP
        EXECUTE format('ALTER TABLE qtss_setups DROP CONSTRAINT %I', cname);
    END LOOP;

    ALTER TABLE qtss_setups
        ADD CONSTRAINT qtss_setups_close_reason_chk
        CHECK (close_reason IS NULL OR close_reason IN
            ('tp_final','sl_hit','trail_stop','invalidated',
             'cancelled','scratch','early_warning',
             'time_stop','hard_invalidation'));
END $$;

-- (b) Time Stop config keys -------------------------------------------
SELECT _qtss_register_key(
    'position_guard.time_stop.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '',
    'Faz D — Time Stop. True iken setup hedeflenen sürede TP1 veya TP_final''e ulaşmazsa kapatılır.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_d','position_guard','time_stop']
);
SELECT _qtss_register_key(
    'position_guard.time_stop.max_bars_pre_tp1', 'setup_engine', 'position_guard',
    'int', '24'::jsonb, '',
    'TP1 alınmamışken setup''un canlı kalabileceği maksimum bar sayısı (setup''un kendi interval''ı cinsinden). 0 → devre dışı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_d','time_stop']
);
SELECT _qtss_register_key(
    'position_guard.time_stop.max_bars_post_tp1', 'setup_engine', 'position_guard',
    'int', '48'::jsonb, '',
    'TP1 alındıktan sonra setup''un canlı kalabileceği ek bar sayısı. 0 → TP1 sonrası zaman baskısı uygulanmaz (yalnızca koruma ratchet çalışır).',
    'number', false, 'normal', ARRAY['setup_engine','faz_d','time_stop']
);

-- (c) Hard Invalidation config ----------------------------------------
SELECT _qtss_register_key(
    'position_guard.hard_invalidation.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '',
    'Faz E — Hard Invalidation. True iken fiyat, detection''ın invalidation_price (D-point / pattern stop) seviyesini aşarsa setup ayrı bir `hard_invalidation` sebebiyle kapatılır. Koruma BE''ye çekilmiş olsa bile formasyon seviyesinde ölüm ayrıca işaretlenir (setup üretimi aynı pivotlardan tekrarlanmaz).',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_e','hard_invalidation']
);
