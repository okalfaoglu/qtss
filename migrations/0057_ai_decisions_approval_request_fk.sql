-- Link `ai_decisions` to `ai_approval_requests` (was part of wrongly numbered `0013_ai_*`, duplicate of v13).
-- `0038_ai_approval_requests.sql` + `0042_ai_engine_tables.sql` own the base tables; this is the delta only.
-- Idempotent: safe if the column already exists (e.g. DB that applied the old `0013_ai` file).

ALTER TABLE ai_decisions
    ADD COLUMN IF NOT EXISTS approval_request_id UUID REFERENCES ai_approval_requests (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_ai_decisions_approval_request ON ai_decisions (approval_request_id)
WHERE
    approval_request_id IS NOT NULL;
