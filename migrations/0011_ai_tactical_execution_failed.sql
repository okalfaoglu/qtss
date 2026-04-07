-- Allow tactical executor to stop retrying after a definitive live execution failure.
ALTER TABLE ai_tactical_decisions DROP CONSTRAINT IF EXISTS ai_tactical_status_chk;
ALTER TABLE ai_tactical_decisions ADD CONSTRAINT ai_tactical_status_chk CHECK (
    status IN (
        'pending_approval',
        'approved',
        'applied',
        'rejected',
        'expired',
        'execution_failed'
    )
);
