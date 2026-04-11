-- D/T/Q config entries: risk mode mapping, TP override, Q-RADAR settings.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('setup', 'risk_mode.regime_map', '{"trending_up":"risk_on","trending_down":"risk_on","ranging":"risk_neutral","squeeze":"risk_neutral","volatile":"risk_off","uncertain":"risk_off"}',
   'Maps RegimeKind to risk_mode (risk_on/risk_neutral/risk_off)'),

  ('setup', 'risk_mode.d.risk_on', '"active"', 'D-profile: risk-on behavior'),
  ('setup', 'risk_mode.d.risk_neutral', '"continue"', 'D-profile: risk-neutral → keep existing, allow strong new'),
  ('setup', 'risk_mode.d.risk_off', '"selective"', 'D-profile: risk-off → only strong structures'),
  ('setup', 'risk_mode.t.risk_on', '"active"', 'T-profile: risk-on → active LONG'),
  ('setup', 'risk_mode.t.risk_neutral', '"selective"', 'T-profile: risk-neutral → selective LONG'),
  ('setup', 'risk_mode.t.risk_off', '"stopped"', 'T-profile: risk-off → stopped/minimal'),
  ('setup', 'risk_mode.q.risk_on', '"active"', 'Q-profile: risk-on behavior'),
  ('setup', 'risk_mode.q.risk_neutral', '"selective"', 'Q-profile: risk-neutral → selective'),
  ('setup', 'risk_mode.q.risk_off', '"selective"', 'Q-profile: risk-off → very selective'),

  ('setup', 'risk_mode.selective_guven_mult', '"1.3"', 'Guven threshold multiplier in selective mode'),

  ('setup', 'tp_override.enabled', '"true"', 'Allow TP to be exceeded when structure is strong'),
  ('setup', 'tp_override.guven_threshold', '"0.60"', 'Min guven to keep position open past TP'),
  ('setup', 'tp_override.max_extension_r', '"3.0"', 'Max R beyond target before forced close'),

  ('setup', 'q_radar.total_capital', '"1500000"', 'Q-RADAR virtual portfolio capital (TL)'),
  ('setup', 'q_radar.max_position_pct', '"15.0"', 'Max % of portfolio per single position'),
  ('setup', 'q_radar.max_total_allocated_pct', '"80.0"', 'Max % of portfolio allocated at once'),
  ('setup', 'q_radar.add_on_enabled', '"true"', 'Allow add-on buys in profit'),
  ('setup', 'q_radar.add_on_min_unrealized_r', '"1.0"', 'Min unrealized R before add-on allowed'),
  ('setup', 'q_radar.add_on_max_count', '"2"', 'Max add-on buys per position'),
  ('setup', 'q_radar.partial_sell_enabled', '"true"', 'Allow partial sells to realize profit'),
  ('setup', 'q_radar.partial_sell_at_r', '"2.0"', 'R threshold to trigger partial sell'),
  ('setup', 'q_radar.partial_sell_pct', '"33.0"', 'Percentage of position to sell partially')
ON CONFLICT (module, config_key) DO NOTHING;
