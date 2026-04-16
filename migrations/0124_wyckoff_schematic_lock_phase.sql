-- 0124_wyckoff_schematic_lock_phase.sql
--
-- Faz 10 pre-step — stop failing Wyckoff structures on Phase A/B flips.
--
-- Production showed the FAILED bucket dominated by "schematic flipped
-- X → Y via LPS/UTAD" while the structure was still on Phase A or B.
-- That's the tracker auto_reclassify firing on a legitimately provisional
-- direction call. Wyckoff literature doesn't commit to a schematic until
-- Phase C (Spring/UTAD). This key lets the orchestrator defer the failure
-- verdict to the configured lock phase; before it, `auto_reclassify` is
-- allowed to mutate the schematic silently.
--
-- Accepted values: A|B|C|D|E (case-insensitive). Default C matches the
-- textbook commitment point.

SELECT _qtss_register_key(
    'wyckoff.schematic_lock_phase',
    'detector',
    'wyckoff',
    'string',
    '"C"'::jsonb,
    '',
    'Earliest Wyckoff phase at which a schematic family flip (bull<->bear) is treated as a FAILED structure. Before this phase, auto_reclassify mutates the schematic silently. Values: A|B|C|D|E.',
    'phase',
    false,
    'normal',
    ARRAY['detector','wyckoff','schematic_lock']
);
