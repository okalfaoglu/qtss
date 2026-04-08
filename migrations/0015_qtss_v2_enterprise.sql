-- 0015_qtss_v2_enterprise.sql
--
-- Faz 0.4 — Enterprise foundation tables for QTSS v2:
--   * qtss_audit_log         : append-only, hash-chained event log
--   * secrets_vault     : encrypted secret storage (envelope encryption)
--   * qtss_users / qtss_roles /
--     qtss_user_roles /
--     qtss_sessions          : minimal RBAC + session model
--
-- Hash chain rationale: each row carries prev_hash + row_hash = sha256(prev_hash || canonical_payload).
-- A verifier walks the chain in insertion order; any tampering breaks the link. The trigger
-- here only enforces append-only — the actual hash is computed in qtss-audit before insert,
-- so the DB stays simple and the crypto stays in one Rust place.

BEGIN;

-- ---------------------------------------------------------------------------
-- qtss_audit_log
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS qtss_audit_log (
    id              BIGSERIAL PRIMARY KEY,
    at              TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    actor           TEXT         NOT NULL,           -- user id, service name, or "system"
    action          TEXT         NOT NULL,           -- e.g. "config.set", "intent.approve"
    subject         TEXT         NOT NULL,           -- entity id the action targets
    payload         JSONB        NOT NULL DEFAULT '{}'::jsonb,
    correlation_id  UUID         NULL,
    prev_hash       BYTEA        NULL,               -- NULL only for the genesis row
    row_hash        BYTEA        NOT NULL,
    CONSTRAINT audit_log_row_hash_unique UNIQUE (row_hash)
);

CREATE INDEX IF NOT EXISTS audit_log_at_idx        ON qtss_audit_log (at);
CREATE INDEX IF NOT EXISTS audit_log_actor_idx     ON qtss_audit_log (actor);
CREATE INDEX IF NOT EXISTS audit_log_action_idx    ON qtss_audit_log (action);
CREATE INDEX IF NOT EXISTS audit_log_subject_idx   ON qtss_audit_log (subject);
CREATE INDEX IF NOT EXISTS audit_log_corr_idx      ON qtss_audit_log (correlation_id);

-- Append-only enforcement: block UPDATE and DELETE at the DB level so even
-- a compromised app role cannot rewrite history without superuser.
CREATE OR REPLACE FUNCTION audit_log_block_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'qtss_audit_log is append-only (% blocked)', TG_OP;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS audit_log_no_update ON qtss_audit_log;
CREATE TRIGGER audit_log_no_update
    BEFORE UPDATE ON qtss_audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_block_mutation();

DROP TRIGGER IF EXISTS audit_log_no_delete ON qtss_audit_log;
CREATE TRIGGER audit_log_no_delete
    BEFORE DELETE ON qtss_audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_block_mutation();

-- ---------------------------------------------------------------------------
-- secrets_vault
-- ---------------------------------------------------------------------------
-- Envelope encryption: each secret is encrypted with a per-row data key (DEK),
-- which is itself wrapped by a master key managed outside the DB (KMS / file).
-- The vault only stores the wrapped DEK + ciphertext + nonce; the master key
-- never touches Postgres. Rotation happens by re-wrapping DEKs.
CREATE TABLE IF NOT EXISTS secrets_vault (
    id              BIGSERIAL PRIMARY KEY,
    name            TEXT        NOT NULL UNIQUE,        -- e.g. "binance.api_key.live"
    description     TEXT        NULL,
    wrapped_dek     BYTEA       NOT NULL,                -- DEK encrypted by master key
    ciphertext      BYTEA       NOT NULL,                -- secret encrypted by DEK
    nonce           BYTEA       NOT NULL,
    kek_version     INT         NOT NULL DEFAULT 1,      -- which master key wrapped the DEK
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rotated_at      TIMESTAMPTZ NULL,
    created_by      TEXT        NOT NULL DEFAULT 'system'
);

CREATE INDEX IF NOT EXISTS secrets_vault_kek_version_idx ON secrets_vault (kek_version);

-- ---------------------------------------------------------------------------
-- RBAC: qtss_roles, qtss_users, qtss_user_roles, qtss_sessions
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS qtss_roles (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,        -- e.g. "admin", "trader", "viewer"
    description TEXT NULL
);

CREATE TABLE IF NOT EXISTS qtss_users (
    id            BIGSERIAL PRIMARY KEY,
    username      TEXT        NOT NULL UNIQUE,
    email         TEXT        NULL UNIQUE,
    password_hash TEXT        NOT NULL,        -- argon2id hash
    is_active     BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at TIMESTAMPTZ NULL
);

CREATE TABLE IF NOT EXISTS qtss_user_roles (
    user_id  BIGINT NOT NULL REFERENCES qtss_users(id) ON DELETE CASCADE,
    role_id  INT    NOT NULL REFERENCES qtss_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

CREATE TABLE IF NOT EXISTS qtss_sessions (
    id          UUID        PRIMARY KEY,
    user_id     BIGINT      NOT NULL REFERENCES qtss_users(id) ON DELETE CASCADE,
    issued_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ NULL,
    user_agent  TEXT        NULL,
    ip_addr     INET        NULL
);

CREATE INDEX IF NOT EXISTS sessions_user_idx     ON qtss_sessions (user_id);
CREATE INDEX IF NOT EXISTS sessions_expires_idx  ON qtss_sessions (expires_at);

-- Seed canonical qtss_roles. Permissions live in qtss-auth (code) keyed by role name,
-- not in DB rows — keeps role->permission edits a code review instead of an
-- ad-hoc UPDATE.
INSERT INTO qtss_roles (name, description) VALUES
    ('admin',  'Full access including config edits and user management'),
    ('trader', 'Can place/approve intents and view all data'),
    ('viewer', 'Read-only access to dashboards and reports')
ON CONFLICT (name) DO NOTHING;

COMMIT;
