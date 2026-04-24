-- Secret access audit log — PR-SEC1 / PR-SEC3.
--
-- Every read of a vault-backed secret (via `qtss-secrets::VaultReader`)
-- is logged here with the caller identity, the secret name (never the
-- plaintext), the outcome (hit / miss / error), and an optional reason
-- tag. Hot path: one row per successful secret resolution — the loops
-- that use long-lived API keys should cache the plaintext in-process
-- rather than hitting the vault on every tick.

CREATE TABLE IF NOT EXISTS secret_access_log (
    id          BIGSERIAL     PRIMARY KEY,
    occurred_at TIMESTAMPTZ   NOT NULL DEFAULT now(),
    actor       TEXT          NOT NULL,      -- worker / api / cli component name
    secret_name TEXT          NOT NULL,      -- must match secrets_vault.name
    outcome     TEXT          NOT NULL
                CHECK (outcome IN ('hit', 'miss_fallback_config', 'miss', 'error')),
    reason      TEXT,                         -- e.g. "anthropic_chat", "telegram_setup_notify"
    kek_version INT,                          -- nullable for miss / error outcomes
    error_msg   TEXT
);

CREATE INDEX IF NOT EXISTS secret_access_log_occurred_at_idx
    ON secret_access_log (occurred_at DESC);
CREATE INDEX IF NOT EXISTS secret_access_log_secret_name_idx
    ON secret_access_log (secret_name, occurred_at DESC);

COMMENT ON TABLE secret_access_log IS
    'PR-SEC1/SEC3 audit trail for vault reads. One row per qtss-secrets::VaultReader::resolve() call. Ciphertext never written — only metadata.';

-- Bootstrap config — KEK version + fallback policy. Actual KEK key
-- material comes from the `QTSS_SECRET_KEK_V1` env var (32 raw bytes
-- hex-encoded), *not* from this table.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('secrets', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master on/off for the vault resolver. If false, every lookup goes straight to system_config (legacy path).'),
    ('secrets', 'kek_version',
     '{"value": 1}'::jsonb,
     'Currently active KEK version. Must match the version provided by QTSS_SECRET_KEK_V<N> env var.'),
    ('secrets', 'allow_config_fallback',
     '{"enabled": true}'::jsonb,
     'When true, a vault miss transparently falls back to system_config (with a warning + audit row). Set false after migrating every secret to enforce vault-only reads.')
ON CONFLICT (module, config_key) DO NOTHING;
