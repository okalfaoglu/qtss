-- 0066_wyckoff_literature_tp_gates.sql — Faz 10 / P0 düzeltmeler.
--
-- Two literature-grade corrections for the Wyckoff trade planner:
--
-- 1) min_net_rr: 1.0 → 1.5
--    Pruden ("Three Skills of Top Trading") and Weis ("Trades About to
--    Happen") both require R:R >= 1.5 as the minimum floor for Wyckoff
--    setups. 1.0 was letting through thin, commission-dominated trades.
--
-- 2) range_cap_factor: 1.5 → 2.0
--    Canonical TP ladder is 0.5x / 1.0x / 2.0x range_height measured
--    from the breakout (creek for longs, ice for shorts). The cap of
--    1.5 clipped legitimate TP3 "extended" targets below the
--    literature value. The classical P&F horizontal count still
--    serves as an upper safety bound when enabled.
--
-- These are UPDATEs (not fresh inserts) because the rows were seeded
-- in 0062_wyckoff_setup_engine.sql.

UPDATE system_config
   SET value = '"1.5"',
       description = 'Reject setup if weighted expected net R < N (Pruden/Weis floor = 1.5).'
 WHERE module = 'setup'
   AND config_key = 'wyckoff.tp.min_net_rr';

UPDATE system_config
   SET value = '"2.0"',
       description = 'TP price cannot exceed breakout ± range_height × factor. 2.0 = canonical TP3 extended measured move.'
 WHERE module = 'setup'
   AND config_key = 'wyckoff.tp.range_cap_factor';
