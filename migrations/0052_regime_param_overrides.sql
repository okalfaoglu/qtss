-- Faz 11: Regime Deep — adaptive parameter overrides per regime.

CREATE TABLE IF NOT EXISTS regime_param_overrides (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module      TEXT NOT NULL,
    config_key  TEXT NOT NULL,
    regime      TEXT NOT NULL,
    value       JSONB NOT NULL,
    description TEXT,
    created_at  TIMESTAMPTZ DEFAULT now(),
    updated_at  TIMESTAMPTZ DEFAULT now(),
    UNIQUE(module, config_key, regime)
);

-- Seed default overrides
INSERT INTO regime_param_overrides (module, config_key, regime, value, description) VALUES
  ('risk', 'max_position_pct', 'volatile',      '1.0',  'Volatile: position size 1%'),
  ('risk', 'max_position_pct', 'squeeze',       '1.5',  'Squeeze: position size 1.5%'),
  ('risk', 'max_position_pct', 'trending_up',   '3.0',  'TrendUp: position size 3%'),
  ('risk', 'max_position_pct', 'trending_down', '3.0',  'TrendDown: position size 3%'),
  ('risk', 'max_position_pct', 'ranging',       '2.0',  'Ranging: position size 2%'),
  ('strategy', 'profit_target_mult', 'trending_up',  '3.0', 'TrendUp: wide target'),
  ('strategy', 'profit_target_mult', 'trending_down','3.0', 'TrendDown: wide target'),
  ('strategy', 'profit_target_mult', 'ranging',      '1.5', 'Ranging: narrow target'),
  ('strategy', 'profit_target_mult', 'squeeze',      '2.0', 'Squeeze: medium target'),
  ('strategy', 'stop_loss_mult',     'volatile',     '2.0', 'Volatile: wide stop'),
  ('strategy', 'stop_loss_mult',     'squeeze',      '0.8', 'Squeeze: tight stop'),
  ('strategy', 'stop_loss_mult',     'trending_up',  '1.5', 'TrendUp: medium stop'),
  ('strategy', 'stop_loss_mult',     'ranging',      '1.0', 'Ranging: normal stop')
ON CONFLICT (module, config_key, regime) DO NOTHING;
