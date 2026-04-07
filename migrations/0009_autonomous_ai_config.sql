-- 0009_autonomous_ai_config.sql
-- Move all env-only settings into system_config / app_config so .env needs only DATABASE_URL.
-- Idempotent: ON CONFLICT DO UPDATE ensures re-runs overwrite with the latest desired values.

-- ============================================================================
-- 1. app_config: ai_engine_config — enable auto-approve at 60% confidence
-- ============================================================================
INSERT INTO app_config (key, value, description)
VALUES (
    'ai_engine_config',
    '{
        "enabled": true,
        "tactical_layer_enabled": true,
        "operational_layer_enabled": true,
        "strategic_layer_enabled": false,
        "auto_approve_enabled": true,
        "auto_approve_threshold": 0.55,
        "require_min_confidence": 0.50,
        "tactical_tick_secs": 900,
        "operational_tick_secs": 120,
        "strategic_tick_secs": 86400,
        "provider_tactical": "anthropic",
        "provider_operational": "anthropic",
        "provider_strategic": "anthropic",
        "model_tactical": "claude-haiku-4-5-20251001",
        "model_operational": "claude-haiku-4-5-20251001",
        "model_strategic": "claude-sonnet-4-20250514",
        "max_tokens_tactical": 4096,
        "max_tokens_operational": 512,
        "max_tokens_strategic": 4096,
        "decision_ttl_secs": 1800
    }'::jsonb,
    'AI engine master config — auto-approve at 55%, persist at 50%'
)
ON CONFLICT (key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 2. system_config: AI tactical executor — dry mode enabled
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'ai_tactical_executor_enabled', '{"enabled": true}'::jsonb,
     'Master switch for AI tactical executor loop'),
    ('worker', 'ai_tactical_executor_dry', '{"enabled": true}'::jsonb,
     'Paper (dry) execution for AI tactical decisions'),
    ('worker', 'ai_tactical_executor_live', '{"enabled": false}'::jsonb,
     'Live exchange execution (off while dry is on)')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 3. system_config: Position manager — dry SL/TP monitoring
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'position_manager_enabled', '{"enabled": true}'::jsonb,
     'Position manager loop: SL/TP monitoring'),
    ('worker', 'position_manager_dry_close_enabled', '{"enabled": true}'::jsonb,
     'Dry (paper) close when SL/TP hit'),
    ('worker', 'position_manager_live_close_enabled', '{"enabled": false}'::jsonb,
     'Live close (off while dry close is on)')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 4. system_config: Notify outbox consumer
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'notify_outbox_enabled', '{"enabled": true}'::jsonb,
     'Enable notify_outbox consumer loop (drains pending → Telegram/webhook)')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 5. system_config: Paper position fill notifications
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'paper_position_notify_enabled', '{"enabled": true}'::jsonb,
     'Paper (dry) fill notifications via Telegram'),
    ('worker', 'paper_position_notify_channels_csv', '{"value": "telegram"}'::jsonb,
     'Channels for paper fill notifications (csv)')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 6. system_config: Hourly position status reports + close results
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'position_status_notify_enabled', '{"enabled": true}'::jsonb,
     'Hourly open-position status reports + close-result messages'),
    ('worker', 'position_status_notify_channels_csv', '{"value": "telegram"}'::jsonb,
     'Channels for position status reports (csv)')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- ============================================================================
-- 7. system_config: Intake auto-promote
-- ============================================================================
INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('worker', 'intake_auto_promote_enabled', '{"enabled": true}'::jsonb,
     'Auto-promote intake playbook candidates to engine_symbols')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();
