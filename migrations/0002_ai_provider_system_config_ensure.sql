-- qtss-ai loads provider endpoints and secrets from `system_config` where module = 'ai'
-- (see crates/qtss-ai/src/provider_secrets.rs). Baseline 0001 seeds these rows; this file
-- is idempotent for databases restored without that seed block or with partial imports.
--
-- Operators set the Anthropic key in JSON shape {"value":"<key>"} via Admin UI or UPDATE
-- (see docs/sql/ai_provider_system_config_ensure.sql). Never commit real API keys.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
    ('ai', 'anthropic_api_key', '{"value":""}', 'ANTHROPIC_API_KEY', true),
    ('ai', 'anthropic_base_url', '{"value":"https://api.anthropic.com"}', NULL, false),
    ('ai', 'anthropic_timeout_secs', '{"secs":120}', NULL, false),
    ('ai', 'ollama_base_url', '{"value":"http://127.0.0.1:11434"}', NULL, false),
    ('ai', 'openai_compat_base_url', '{"value":""}', 'OpenAI-compatible /v1 base', false),
    ('ai', 'openai_compat_headers_json', '{"value":""}', 'Extra JSON headers', false),
    ('ai', 'onprem_timeout_secs', '{"secs":180}', NULL, false),
    ('ai', 'onprem_max_in_flight', '{"value":"4"}', NULL, false),
    ('ai', 'onprem_api_key', '{"value":""}', 'Optional Bearer for gateway', true),
    ('ai', 'gemini_api_key', '{"value":""}', 'Optional; if empty qtss-ai uses telegram_setup_analysis.gemini_api_key', true),
    ('ai', 'gemini_api_root', '{"value":""}', 'Empty = https://generativelanguage.googleapis.com/v1beta', false),
    ('ai', 'gemini_timeout_secs', '{"secs":120}', 'generateContent HTTP timeout floor', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- Align `app_config.ai_engine_config` with qtss-ai::AiEngineConfig when older JSON omits output_locale.
UPDATE app_config
SET value = value || jsonb_build_object('output_locale', NULL)
WHERE key = 'ai_engine_config'
  AND NOT (value ? 'output_locale');
