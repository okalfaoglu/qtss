-- 0173_faz_9b_model_roles.sql
--
-- Faz 9B Kalem H — model role registry.
--
-- Today qtss_models has a single bool `active`, enforced by a partial
-- unique index (one active per family). This is fine for the single-
-- champion path but doesn't express "shadow" (scored alongside the
-- champion for calibration/eval without gating setups) or "challenger"
-- (A/B split, partial traffic). A TEXT role column gives us a stable
-- vocabulary the sidecar can extend later without another migration.
--
-- Roles:
--   'active'    — the model the inference gate serves; one per family
--   'shadow'    — scored in parallel, not binding; any number per family
--   'archived'  — historic rows; default for everything else
--
-- `active` bool is retained for backward compatibility and kept in sync
-- via a BEFORE INSERT/UPDATE trigger so no caller needs to update both.
--
-- Idempotent.

ALTER TABLE qtss_models
  ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'archived'
    CHECK (role IN ('active','shadow','archived'));

-- Backfill: preserve existing `active=true` rows as role='active';
-- everything else becomes 'archived' (matches default).
UPDATE qtss_models SET role = 'active'   WHERE active = true   AND role <> 'active';
UPDATE qtss_models SET role = 'archived' WHERE active = false  AND role = 'active';

-- One active row per family — reuse the existing partial unique; add a
-- role-expressed partial unique so both stay truthful.
CREATE UNIQUE INDEX IF NOT EXISTS qtss_models_one_role_active
  ON qtss_models(model_family)
  WHERE role = 'active';

-- Keep `active` and `role` in lock-step. Simpler than rewiring every
-- reader; the bool stays the fast path for inference joins, role is
-- the source of truth for the GUI.
CREATE OR REPLACE FUNCTION qtss_models_sync_active_role()
RETURNS trigger AS $$
BEGIN
  -- If the caller updated `role`, derive `active` from it.
  IF TG_OP = 'INSERT' OR NEW.role IS DISTINCT FROM OLD.role THEN
    NEW.active := (NEW.role = 'active');
  -- If the caller only flipped `active`, map it back to role.
  ELSIF NEW.active IS DISTINCT FROM OLD.active THEN
    NEW.role := CASE WHEN NEW.active THEN 'active' ELSE 'archived' END;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_qtss_models_sync_active_role ON qtss_models;
CREATE TRIGGER trg_qtss_models_sync_active_role
  BEFORE INSERT OR UPDATE ON qtss_models
  FOR EACH ROW EXECUTE FUNCTION qtss_models_sync_active_role();

COMMENT ON COLUMN qtss_models.role IS
  'Faz 9B Kalem H — active | shadow | archived. Source of truth for the GUI registry. `active` bool stays in sync via trigger.';
