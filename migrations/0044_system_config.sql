-- FAZ 11.1 — Operational parameters in DB (module-scoped); secrets stay in env / secret store (see docs/CONFIG_REGISTRY.md).

CREATE TABLE system_config (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module TEXT NOT NULL,
    config_key TEXT NOT NULL,
    value JSONB NOT NULL DEFAULT '{}'::jsonb,
    schema_version INT NOT NULL DEFAULT 1,
    description TEXT,
    is_secret BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_user_id UUID REFERENCES users (id),
    CONSTRAINT system_config_module_key_unique UNIQUE (module, config_key)
);

CREATE INDEX idx_system_config_module ON system_config (module);

CREATE INDEX idx_system_config_module_config_key ON system_config (module, config_key);

-- Idempotent seed (non-secret documentation defaults).
INSERT INTO
    system_config (module, config_key, value, description)
VALUES (
        'ai',
        'worker_doc',
        '{"note":"QTSS_AI_ENGINE_WORKER=0 disables qtss-worker AI spawn loops; providers still need keys when enabled."}'::jsonb,
        'AI worker env cross-reference (FAZ 5 / 8).'
    )
ON CONFLICT (module, config_key) DO NOTHING;
