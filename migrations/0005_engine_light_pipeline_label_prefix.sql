-- Engine analysis: skip classic formations + TBM formation pillar boost for labels matching this prefix (default intake:).

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES (
        'worker',
        'engine_light_pipeline_label_prefix',
        '{"value":"intake:"}'::jsonb,
        'engine_loop: label prefix → light pipeline (no formations scan; TBM structure without pattern boost)',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
