-- 0037 — system_config_audit: immutable change log + auto-trigger
-- Every INSERT/UPDATE/DELETE on system_config is captured here.

CREATE TABLE IF NOT EXISTS system_config_audit (
    id          BIGSERIAL PRIMARY KEY,
    module      TEXT NOT NULL,
    config_key  TEXT NOT NULL,
    action      TEXT NOT NULL CHECK (action IN ('create','update','delete','rollback')),
    old_value   JSONB,
    new_value   JSONB,
    changed_by  UUID REFERENCES users (id) ON DELETE SET NULL,
    changed_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sc_audit_key
    ON system_config_audit (module, config_key, changed_at DESC);

-- Trigger function: capture every mutation automatically.
CREATE OR REPLACE FUNCTION fn_system_config_audit() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO system_config_audit (module, config_key, action, new_value, changed_by)
        VALUES (NEW.module, NEW.config_key, 'create', NEW.value, NEW.updated_by_user_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        -- Only log if value actually changed.
        IF OLD.value IS DISTINCT FROM NEW.value THEN
            INSERT INTO system_config_audit (module, config_key, action, old_value, new_value, changed_by)
            VALUES (NEW.module, NEW.config_key, 'update', OLD.value, NEW.value, NEW.updated_by_user_id);
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO system_config_audit (module, config_key, action, old_value, changed_by)
        VALUES (OLD.module, OLD.config_key, 'delete', OLD.value, OLD.updated_by_user_id);
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_system_config_audit ON system_config;
CREATE TRIGGER trg_system_config_audit
    AFTER INSERT OR UPDATE OR DELETE ON system_config
    FOR EACH ROW EXECUTE FUNCTION fn_system_config_audit();
