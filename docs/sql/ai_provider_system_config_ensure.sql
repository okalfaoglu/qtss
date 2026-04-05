-- Manual / operator SQL: ensure AI provider rows exist and document Anthropic key storage.
-- Same logical statements as migrations/0002_ai_provider_system_config_ensure.sql (run via API/worker migrate).
-- Use psql or Admin tooling; do not commit secrets into git.

-- Idempotent: create missing system_config rows (module ai).
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
    ('ai', 'onprem_api_key', '{"value":""}', 'Optional Bearer for gateway', true)
ON CONFLICT (module, config_key) DO NOTHING;

-- Optional: add output_locale to ai_engine_config when the key is missing (matches qtss-ai serde defaults).
UPDATE app_config
SET value = value || jsonb_build_object('output_locale', NULL)
WHERE key = 'ai_engine_config'
  AND NOT (value ? 'output_locale');

-- Set Anthropic API key (replace the placeholder; prefer dollar-quoting if the key contains quotes).
-- UPDATE system_config
-- SET value = jsonb_build_object('value', $k$PASTE_ANTHROPIC_KEY_HERE$k$),
--     updated_at = now()
-- WHERE module = 'ai' AND config_key = 'anthropic_api_key';
