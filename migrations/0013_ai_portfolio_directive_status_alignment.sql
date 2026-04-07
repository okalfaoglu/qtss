-- Align legacy ai_portfolio_directives.status with parent ai_decisions (strategic layer + fetch_active_portfolio_directive JOIN).

UPDATE ai_portfolio_directives p
SET status = 'pending_approval'
FROM ai_decisions d
WHERE p.decision_id = d.id
  AND d.status = 'pending_approval'
  AND p.status = 'active';

UPDATE ai_portfolio_directives p
SET status = 'rejected'
FROM ai_decisions d
WHERE p.decision_id = d.id
  AND d.status = 'rejected'
  AND p.status IN ('active', 'pending_approval', 'approved');

UPDATE ai_portfolio_directives p
SET status = 'expired'
FROM ai_decisions d
WHERE p.decision_id = d.id
  AND d.status = 'expired'
  AND p.status IN ('active', 'pending_approval', 'approved');

UPDATE ai_portfolio_directives p
SET status = 'approved'
FROM ai_decisions d
WHERE p.decision_id = d.id
  AND d.status IN ('approved', 'applied')
  AND p.status = 'active';

ALTER TABLE ai_portfolio_directives
  ALTER COLUMN status SET DEFAULT 'pending_approval';
