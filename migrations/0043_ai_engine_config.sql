-- FAZ 1.6 — Seed `app_config.ai_engine_config` (idempotent).

INSERT INTO
    app_config (
        key,
        value,
        description,
        updated_by_user_id
    )
VALUES (
        'ai_engine_config',
        '{"enabled": false, "tactical_layer_enabled": true, "operational_layer_enabled": true, "strategic_layer_enabled": false, "auto_approve_threshold": 0.85, "auto_approve_enabled": false, "tactical_tick_secs": 900, "operational_tick_secs": 120, "strategic_tick_secs": 86400, "provider_tactical": "anthropic", "provider_operational": "anthropic", "provider_strategic": "anthropic", "model_tactical": "claude-haiku-4-5-20251001", "model_operational": "claude-haiku-4-5-20251001", "model_strategic": "claude-sonnet-4-20250514", "max_tokens_tactical": 1024, "max_tokens_operational": 512, "max_tokens_strategic": 4096, "decision_ttl_secs": 1800, "require_min_confidence": 0.60}'::jsonb,
        'AI engine defaults (providers + ticks); qtss-ai reads with app_config merge / env overrides.',
        NULL
    )
ON CONFLICT (key) DO NOTHING;
