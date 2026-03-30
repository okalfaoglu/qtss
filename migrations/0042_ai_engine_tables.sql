-- FAZ 1 — AI engine core tables (QTSS_MASTER_DEV_GUIDE §4 FAZ 1.1–1.5).
-- Parent: ai_decisions. Children: tactical / position_directives / portfolio_directives / outcomes.

CREATE TABLE ai_decisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    layer TEXT NOT NULL,
    symbol TEXT,
    model_id TEXT,
    prompt_hash TEXT,
    input_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb,
    raw_output TEXT,
    parsed_decision JSONB,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    approved_by TEXT,
    approved_at TIMESTAMPTZ,
    applied_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    confidence DOUBLE PRECISION,
    meta_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT ai_decisions_layer_chk CHECK (
        layer IN ('strategic', 'tactical', 'operational')
    ),
    CONSTRAINT ai_decisions_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired',
            'error'
        )
    )
);

CREATE INDEX idx_ai_decisions_symbol_layer_created ON ai_decisions (
    symbol,
    layer,
    created_at DESC
);

CREATE INDEX idx_ai_decisions_status_pending ON ai_decisions (status)
WHERE
    status IN ('pending_approval', 'approved');

CREATE TABLE ai_tactical_decisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ NOT NULL,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    position_size_multiplier DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    entry_price_hint DOUBLE PRECISION,
    stop_loss_pct DOUBLE PRECISION,
    take_profit_pct DOUBLE PRECISION,
    reasoning TEXT,
    confidence DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    CONSTRAINT ai_tactical_direction_chk CHECK (
        direction IN (
            'strong_buy',
            'buy',
            'neutral',
            'sell',
            'strong_sell',
            'no_trade'
        )
    ),
    CONSTRAINT ai_tactical_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired'
        )
    )
);

CREATE INDEX idx_ai_tactical_symbol_status_created ON ai_tactical_decisions (
    symbol,
    status,
    created_at DESC
);

CREATE INDEX idx_ai_tactical_decision_id ON ai_tactical_decisions (decision_id);

CREATE TABLE ai_position_directives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    symbol TEXT NOT NULL,
    open_position_ref UUID,
    action TEXT NOT NULL,
    new_stop_loss_pct DOUBLE PRECISION,
    new_take_profit_pct DOUBLE PRECISION,
    trailing_callback_pct DOUBLE PRECISION,
    partial_close_pct DOUBLE PRECISION,
    reasoning TEXT,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    CONSTRAINT ai_position_directives_action_chk CHECK (
        action IN (
            'keep',
            'tighten_stop',
            'widen_stop',
            'activate_trailing',
            'deactivate_trailing',
            'partial_close',
            'full_close',
            'add_to_position'
        )
    ),
    CONSTRAINT ai_position_directives_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired'
        )
    )
);

CREATE INDEX idx_ai_position_directives_decision_id ON ai_position_directives (decision_id);

CREATE INDEX idx_ai_position_directives_symbol_status_created ON ai_position_directives (
    symbol,
    status,
    created_at DESC
);

CREATE TABLE ai_portfolio_directives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ,
    risk_budget_pct DOUBLE PRECISION,
    max_open_positions INT,
    preferred_regime TEXT,
    symbol_scores JSONB NOT NULL DEFAULT '{}'::jsonb,
    macro_note TEXT,
    status TEXT NOT NULL DEFAULT 'active'
);

CREATE INDEX idx_ai_portfolio_directives_decision_id ON ai_portfolio_directives (decision_id);

CREATE INDEX idx_ai_portfolio_directives_status_created ON ai_portfolio_directives (status, created_at DESC);

CREATE TABLE ai_decision_outcomes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    pnl_pct DOUBLE PRECISION,
    pnl_usdt DOUBLE PRECISION,
    outcome TEXT NOT NULL,
    holding_hours DOUBLE PRECISION,
    notes TEXT,
    CONSTRAINT ai_decision_outcomes_outcome_chk CHECK (
        outcome IN (
            'profit',
            'loss',
            'breakeven',
            'expired_unused'
        )
    )
);

CREATE INDEX idx_ai_decision_outcomes_decision_id ON ai_decision_outcomes (decision_id);

CREATE INDEX idx_ai_decision_outcomes_recorded ON ai_decision_outcomes (recorded_at DESC);
