-- ACP Pine parity: Trendoscope `getZigzagAndPattern` walks zigzag levels until pivot floor breaks.
-- `max_zigzag_levels: 0` means unlimited in qtss-chart-patterns; old app_config seed used 2.
-- Only bump installs that still have the legacy default (2) so explicit "2" user overrides stay if we cannot distinguish.
UPDATE app_config
SET value = jsonb_set(value, '{scanning,max_zigzag_levels}', '0'::jsonb, true)
WHERE key = 'acp_chart_patterns'
  AND COALESCE((value #>> '{scanning,max_zigzag_levels}')::int, -1) = 2;
