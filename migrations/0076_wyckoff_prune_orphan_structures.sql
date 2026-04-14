-- 0076_wyckoff_prune_orphan_structures.sql — Faz 10 / P10.
--
-- Delete `wyckoff_structures` rows that were spawned from a bare
-- Phase C/D/E event without any Phase A seed (PS/SC/BC/AR/ST).
--
-- Root cause: the bootstrap-promotion rule (22672e4) advances
-- current_phase to the event's canonical phase when the tracker
-- starts mid-structure. Before the orchestrator's
-- `upsert_wyckoff_structure_from_detection` was guarded, the *None*
-- branch would also spawn a fresh row from e.g. an isolated SOS,
-- producing a "Phase D without A/B/C" record. The orchestrator guard
-- is now in place (only Phase A events seed new structures), so the
-- remaining stale rows here are removed one-shot.

DELETE FROM wyckoff_structures
 WHERE current_phase IN ('C','D','E')
   AND NOT EXISTS (
     SELECT 1 FROM jsonb_array_elements(events_json) e
      WHERE e->>'event' IN ('p_s','PS','s_c','SC','b_c','BC','a_r','AR','s_t','ST')
   );
