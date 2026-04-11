-- 0047: Commission gate — reject setups where profit < commission cost.

-- Widen the CHECK constraint to allow 'commission_gate' rejection reason.
ALTER TABLE qtss_v2_setup_rejections
    DROP CONSTRAINT IF EXISTS qtss_v2_setup_rejections_reject_reason_check;

ALTER TABLE qtss_v2_setup_rejections
    ADD CONSTRAINT qtss_v2_setup_rejections_reject_reason_check
    CHECK (reject_reason IN (
        'total_risk_cap','max_concurrent','correlation_cap','commission_gate'
    ));

-- Default commission config (taker bps — worst case for market orders).
INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('setup', 'commission.taker_bps', '5', 'Default taker commission in basis points (round-trip = 2x)')
ON CONFLICT (module, config_key) DO NOTHING;
