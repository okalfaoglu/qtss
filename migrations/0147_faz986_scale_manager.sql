-- Faz 9.8.6 — Scale manager config keys.
--
-- Feed `qtss_risk::scale_manager::ScaleManagerConfig`. Worker loader
-- builds the struct from these; Config Editor edits live (CLAUDE.md #2).

SELECT _qtss_register_key(
    'scale_manager.enabled', 'risk', 'scale_manager',
    'bool', 'true'::jsonb, '',
    'Master switch for the post-trade scale manager (pyramid / scale-out / add-on-dip).',
    'bool', true, 'high', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.max_pyramid_legs', 'risk', 'scale_manager',
    'int', '2'::jsonb, '',
    'Maximum extra pyramid legs on top of the original entry.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.pyramid_trigger_r', 'risk', 'scale_manager',
    'float', '1.0'::jsonb, 'R',
    'Unrealised R-multiple required before pyramiding in.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.pyramid_leg_pct', 'risk', 'scale_manager',
    'float', '0.5'::jsonb, '',
    'Add size per pyramid leg as fraction of remaining qty.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.scale_out_r', 'risk', 'scale_manager',
    'float', '2.0'::jsonb, 'R',
    'R-multiple milestone that fires a scale-out leg.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.scale_out_pct', 'risk', 'scale_manager',
    'float', '0.33'::jsonb, '',
    'Reduce size per scale-out milestone as fraction of remaining qty.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.add_on_dip_atr_mult', 'risk', 'scale_manager',
    'float', '1.0'::jsonb, 'atr',
    'Dip depth (vs swing high) in ATR units required to add on a dip.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);

SELECT _qtss_register_key(
    'scale_manager.add_on_dip_pct', 'risk', 'scale_manager',
    'float', '0.25'::jsonb, '',
    'Add size on qualifying dip as fraction of remaining qty.',
    'number', false, 'normal', ARRAY['risk','faz98','scale_manager']
);
