-- FAZ 25.x — single source of truth for account equity.
--
-- User: "sermaye tek kaynak olsun." Earlier we updated four
-- different config keys (radar.default_starting_capital_usd,
-- setup.q_radar.total_capital, worker.kill_switch_reference_equity
-- _usdt, strategy.max_position_notional_usdt) by hand and the
-- numbers immediately drifted again because each consumer reads its
-- own key with its own fallback. This migration adds ONE master
-- key, account.equity_usd, and rewires the legacy keys as soft
-- fallbacks (the resolver in qtss-storage prefers the master, then
-- falls back to the legacy key, then to a built-in default).
--
-- Operators only have to update account.equity_usd from now on.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('account', 'equity_usd',
     '{"value": 1000}'::jsonb,
     'SINGLE source of truth for account equity / starting capital. Every consumer (RADAR aggregator, allocator, kill switch, IQ-D / IQ-T sizing) reads this row first via resolve_account_equity_usd(). Legacy module-specific keys (radar.default_starting_capital_usd, setup.q_radar.total_capital, etc.) remain as soft fallbacks for backward compatibility but should NOT be edited going forward.')
ON CONFLICT (module, config_key) DO UPDATE
   SET value = EXCLUDED.value, updated_at = now();

-- Mirror the master into the legacy keys so any not-yet-refactored
-- consumer reads the same number. A subsequent migration / code
-- patch can drop the legacy keys once every reader is migrated.
UPDATE system_config SET value = '{"value": 1000}'::jsonb, updated_at = now()
  WHERE module='radar' AND config_key='default_starting_capital_usd';
UPDATE system_config SET value = '"1000"'::jsonb, updated_at = now()
  WHERE module='setup' AND config_key='q_radar.total_capital';
UPDATE system_config SET value = '{"value": "1000"}'::jsonb, updated_at = now()
  WHERE module='strategy' AND config_key='max_position_notional_usdt';
UPDATE system_config SET value = '{"value": "1000"}'::jsonb, updated_at = now()
  WHERE module='worker' AND config_key='kill_switch_reference_equity_usdt';
