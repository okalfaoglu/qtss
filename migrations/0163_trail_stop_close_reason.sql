-- 0163_trail_stop_close_reason.sql
--
-- Faz 9.7.5 — extend qtss_setups.close_reason domain with 'trail_stop'
-- so the setup watcher can distinguish stops hit in trailing mode from
-- plain SL hits. CLAUDE.md #2: values come from the canonical enum in
-- qtss-notify::LifecycleEventKind::close_reason.

DO $$
DECLARE
    cname TEXT;
BEGIN
    -- Drop any existing close_reason check constraint on qtss_setups,
    -- regardless of the historical name it was created under (v2 rename).
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
            ('tp_final','sl_hit','trail_stop','invalidated','cancelled'));
END $$;
