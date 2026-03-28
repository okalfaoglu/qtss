-- Trading range: spot tek yönlü (long-only) vs vadeli çift yönlü — `engine_symbols.signal_direction_mode`.
-- Önkoşul: `engine_symbols` tablosu (ör. `0015_engine_analysis.sql`). Değerler: both | long_only | short_only | auto_segment.

ALTER TABLE engine_symbols
ADD COLUMN IF NOT EXISTS signal_direction_mode TEXT NOT NULL DEFAULT 'auto_segment';

COMMENT ON COLUMN engine_symbols.signal_direction_mode IS
    'both | long_only | short_only | auto_segment — worker signal_dashboard ve F1 olaylarında uygulanır';
