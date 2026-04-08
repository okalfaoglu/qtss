-- QTSS v2 — Config Registry (PR-1)
-- See: docs/QTSS_V2_ARCHITECTURE_PLAN.md §7C
--
-- Replaces (eventually, in Faz 7) the legacy `app_config` and `system_config`
-- tables with a normalized, scoped, validated, audited config registry.
-- Both legacy tables are LEFT UNTOUCHED in this migration; v2 runs in parallel.
--
-- Tables introduced:
--   * config_schema  — key catalog (type, validation, default, description)
--   * config_scope   — scope hierarchy (global > asset_class > venue > strategy > instrument)
--   * config_value   — actual override values (versioned, optimistic-locked)
--   * config_audit   — immutable change log (hash chain — implementation in PR-4)
--
-- All DDL is idempotent (IF NOT EXISTS) so this migration is safe to re-run.

-- ---------------------------------------------------------------------------
-- 1. config_schema  — key catalog
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS config_schema (
    key               TEXT PRIMARY KEY,
    category          TEXT NOT NULL,
    subcategory       TEXT,
    value_type        TEXT NOT NULL,
    json_schema       JSONB NOT NULL DEFAULT '{}'::jsonb,
    default_value     JSONB NOT NULL,
    unit              TEXT,
    description       TEXT NOT NULL,
    ui_widget         TEXT,
    requires_restart  BOOLEAN NOT NULL DEFAULT false,
    is_secret_ref     BOOLEAN NOT NULL DEFAULT false,
    sensitivity       TEXT NOT NULL DEFAULT 'normal',
    deprecated_at     TIMESTAMPTZ,
    introduced_in     TEXT,
    tags              TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT config_schema_value_type_chk
        CHECK (value_type IN ('int','float','decimal','string','bool','enum','object','array','duration')),
    CONSTRAINT config_schema_sensitivity_chk
        CHECK (sensitivity IN ('low','normal','high'))
);

CREATE INDEX IF NOT EXISTS idx_config_schema_category
    ON config_schema (category, subcategory);

-- ---------------------------------------------------------------------------
-- 2. config_scope  — hierarchy
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS config_scope (
    id          BIGSERIAL PRIMARY KEY,
    scope_type  TEXT NOT NULL,
    scope_key   TEXT NOT NULL DEFAULT '',
    parent_id   BIGINT REFERENCES config_scope (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT config_scope_type_chk
        CHECK (scope_type IN ('global','asset_class','venue','strategy','instrument','user')),
    CONSTRAINT config_scope_unique UNIQUE (scope_type, scope_key)
);

-- Seed: global scope (id will likely be 1, but code resolves by (type, key)).
INSERT INTO config_scope (scope_type, scope_key)
VALUES ('global', '')
ON CONFLICT (scope_type, scope_key) DO NOTHING;

-- Seed: asset class scopes
INSERT INTO config_scope (scope_type, scope_key) VALUES
    ('asset_class', 'crypto_spot'),
    ('asset_class', 'crypto_futures'),
    ('asset_class', 'equity_bist'),
    ('asset_class', 'equity_nasdaq')
ON CONFLICT (scope_type, scope_key) DO NOTHING;

-- ---------------------------------------------------------------------------
-- 3. config_value  — override values
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS config_value (
    id           BIGSERIAL PRIMARY KEY,
    key          TEXT NOT NULL REFERENCES config_schema (key) ON DELETE CASCADE,
    scope_id     BIGINT NOT NULL REFERENCES config_scope (id) ON DELETE CASCADE,
    value        JSONB NOT NULL,
    version      INT NOT NULL DEFAULT 1,
    enabled      BOOLEAN NOT NULL DEFAULT true,
    valid_from   TIMESTAMPTZ,
    valid_until  TIMESTAMPTZ,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    CONSTRAINT config_value_unique UNIQUE (key, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_config_value_key      ON config_value (key);
CREATE INDEX IF NOT EXISTS idx_config_value_scope    ON config_value (scope_id);
CREATE INDEX IF NOT EXISTS idx_config_value_validity ON config_value (valid_from, valid_until)
    WHERE valid_from IS NOT NULL OR valid_until IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 4. config_audit  — immutable change log (hash chain hooked up in PR-4)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS config_audit (
    id           BIGSERIAL PRIMARY KEY,
    key          TEXT NOT NULL,
    scope_id     BIGINT,
    action       TEXT NOT NULL,
    old_value    JSONB,
    new_value    JSONB,
    changed_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    changed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason       TEXT NOT NULL,
    correlation  UUID,
    hash_prev    BYTEA,
    hash_self    BYTEA,
    CONSTRAINT config_audit_action_chk
        CHECK (action IN ('create','update','delete','rollback','migrated_from_v1'))
);

CREATE INDEX IF NOT EXISTS idx_config_audit_key  ON config_audit (key, changed_at DESC);
CREATE INDEX IF NOT EXISTS idx_config_audit_corr ON config_audit (correlation);

-- ---------------------------------------------------------------------------
-- 5. Hot-reload notification trigger
-- ---------------------------------------------------------------------------
-- Listening channel: 'config_changed'  (payload: jsonb { key, scope_id, action })
CREATE OR REPLACE FUNCTION notify_config_changed() RETURNS trigger AS $$
DECLARE
    payload JSONB;
BEGIN
    payload := jsonb_build_object(
        'key',      COALESCE(NEW.key, OLD.key),
        'scope_id', COALESCE(NEW.scope_id, OLD.scope_id),
        'action',   TG_OP
    );
    PERFORM pg_notify('config_changed', payload::text);
    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_config_value_notify ON config_value;
CREATE TRIGGER trg_config_value_notify
    AFTER INSERT OR UPDATE OR DELETE ON config_value
    FOR EACH ROW EXECUTE FUNCTION notify_config_changed();
