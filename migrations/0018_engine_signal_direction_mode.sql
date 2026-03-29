-- Motor hedefi: `both` | `long_only` | `short_only` | `auto_segment` (`update_engine_symbol_patch`, worker politika).

ALTER TABLE engine_symbols
  ADD COLUMN IF NOT EXISTS signal_direction_mode TEXT NOT NULL DEFAULT 'auto_segment';
