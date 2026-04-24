-- Allocator v2 dual-mode support (PR-FAZ13E extension).
-- Arms parallel dry + live setups per approved signal when operator
-- opts in. Live execution still requires `execution.execution.live.
-- enabled = true` at the execution_bridge gate (two-key safety).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'modes', '{"modes": ["dry"]}'::jsonb,
     'Allocator v2 her onaylı sinyal için hangi modlarda kayıt açsın. ["dry"] = sadece paper; ["dry","live"] = paralel paper + gerçek. Live için ayrıca execution.live.enabled=true ve geçerli exchange_accounts satırı gerekli.'),

    -- execution_bridge live gate — varsayılan false. Kullanıcı
    -- bilinçli açmalı.
    ('execution', 'execution.live.enabled', '{"enabled": false}'::jsonb,
     'execution_bridge live order dispatch ana switch. Allocator ["dry","live"] modunda çalışsa bile bu false ise live order açılmaz (setup rejected).')
ON CONFLICT (module, config_key) DO NOTHING;
