#![allow(dead_code, unused_variables)]

//! Multi-gate AI approval evaluator (Faz 13C).
//!
//! A setup travels through six sequential gates. The first gate to
//! fail rejects the setup and records a machine-readable reason the
//! Telegram card / GUI can display. When every gate passes:
//!   * if `confidence >= auto_approve_threshold`, status → `approved`
//!     (auto_approved = true)
//!   * else status → `pending` for human review
//!
//! Gates (in order):
//!   1. **Confidence** — raw detector structural score
//!   2. **MetaLabel** — ML meta-model probability (bypassed when no
//!      model is loaded → returns 1.0)
//!   3. **Regime** — reject setups generated under blacklisted regimes
//!   4. **Confluence** — aggregated cross-family score from
//!      `confluence_snapshots.confidence`
//!   5. **RiskBudget** — per-symbol daily rejection cap
//!   6. **EventBlackout** — macro calendar proximity
//!
//! Dispatch is a `Vec<Box<dyn Gate>>` so adding gate #7 is one entry
//! (CLAUDE.md #1, dispatch table). Every threshold loaded from
//! `system_config.ai_approval.*` at evaluation time (CLAUDE.md #2).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tag for the first gate that failed. Machine-readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    ConfidenceBelow,
    MetaLabelBelow,
    RegimeUnsupported,
    ConfluenceBelow,
    RiskBudgetExhausted,
    EventBlackout,
}

impl RejectionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfidenceBelow => "confidence_below",
            Self::MetaLabelBelow => "meta_label_below",
            Self::RegimeUnsupported => "regime_unsupported",
            Self::ConfluenceBelow => "confluence_below",
            Self::RiskBudgetExhausted => "risk_budget_exhausted",
            Self::EventBlackout => "event_blackout",
        }
    }
}

/// Per-gate scorecard row surfaced to the UI. `gate` is a plain
/// `String` rather than a `&'static str` because serde's derived
/// `Deserialize` triggers a rustc 1.95 borrow-checker ICE when that
/// combination lands in the same struct as the derived `Serialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateScore {
    pub gate: String,
    pub score: f64,
    pub threshold: f64,
    pub passed: bool,
}

/// Outcome of the full six-gate evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiGateVerdict {
    pub verdict: VerdictStatus,
    pub auto_approved: bool,
    pub rejection_reason: Option<RejectionReason>,
    pub gate_scores: Vec<GateScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictStatus {
    Approved,
    Pending,
    Rejected,
}

impl VerdictStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Pending => "pending",
            Self::Rejected => "rejected",
        }
    }
}

/// Inputs the evaluator needs. Callers (setup_watcher, Telegram
/// webhook handler, execution_bridge pre-flight) build one of these
/// per setup.
#[derive(Debug, Clone)]
pub struct GateContext {
    pub symbol: String,
    pub confidence: f64,
    /// None when no meta-label model is loaded.
    pub meta_label: Option<f64>,
    /// "trending_up" / "ranging" / etc. from the latest regime
    /// snapshot. Defaults to "uncertain" when unavailable.
    pub regime: String,
    /// Confluence engine's normalised 0..1 confidence reading.
    pub confluence: f64,
    /// Rejected setups for this symbol in the last 24h.
    pub rejected_today: i64,
    /// True when the macro calendar has an event within the blackout
    /// window right now.
    pub in_event_blackout: bool,
}

/// Thresholds loaded from `system_config.ai_approval.*` (caller fills
/// this). Defaults mirror the seeded config.
#[derive(Debug, Clone)]
pub struct GateThresholds {
    pub auto_approve_threshold: f64,
    pub min_confidence: f64,
    pub min_meta_label: f64,
    pub min_confluence: f64,
    pub regime_blacklist: Vec<String>,
    pub max_daily_rejected_per_symbol: i64,
    /// Present for completeness — the actual blackout check happens
    /// via `GateContext::in_event_blackout` which the caller fills
    /// from the macro calendar.
    pub event_blackout_minutes: i64,
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            auto_approve_threshold: 0.75,
            min_confidence: 0.65,
            min_meta_label: 0.55,
            min_confluence: 0.60,
            regime_blacklist: vec!["choppy".into(), "uncertain".into()],
            max_daily_rejected_per_symbol: 10,
            event_blackout_minutes: 30,
        }
    }
}

/// Run the full gate chain. Returns the first failing gate or
/// `Approved` / `Pending` when every gate passes.
pub fn evaluate(ctx: &GateContext, thr: &GateThresholds) -> MultiGateVerdict {
    let mut scores = Vec::with_capacity(6);

    // Gate 1 — raw confidence.
    let passed = ctx.confidence >= thr.min_confidence;
    scores.push(GateScore {
        gate: "confidence".into(),
        score: ctx.confidence,
        threshold: thr.min_confidence,
        passed,
    });
    if !passed {
        return reject(RejectionReason::ConfidenceBelow, scores);
    }

    // Gate 2 — meta label (pass-through when absent).
    let meta = ctx.meta_label.unwrap_or(1.0);
    let passed = meta >= thr.min_meta_label;
    scores.push(GateScore {
        gate: "meta_label".into(),
        score: meta,
        threshold: thr.min_meta_label,
        passed,
    });
    if !passed {
        return reject(RejectionReason::MetaLabelBelow, scores);
    }

    // Gate 3 — regime fit.
    let regime_lc = ctx.regime.to_ascii_lowercase();
    let on_blacklist = thr
        .regime_blacklist
        .iter()
        .any(|r| r.to_ascii_lowercase() == regime_lc);
    scores.push(GateScore {
        gate: "regime_fit".into(),
        score: if on_blacklist { 0.0 } else { 1.0 },
        threshold: 1.0,
        passed: !on_blacklist,
    });
    if on_blacklist {
        return reject(RejectionReason::RegimeUnsupported, scores);
    }

    // Gate 4 — confluence score.
    let passed = ctx.confluence >= thr.min_confluence;
    scores.push(GateScore {
        gate: "confluence".into(),
        score: ctx.confluence,
        threshold: thr.min_confluence,
        passed,
    });
    if !passed {
        return reject(RejectionReason::ConfluenceBelow, scores);
    }

    // Gate 5 — risk budget.
    let passed = ctx.rejected_today < thr.max_daily_rejected_per_symbol;
    scores.push(GateScore {
        gate: "risk_budget".into(),
        score: ctx.rejected_today as f64,
        threshold: thr.max_daily_rejected_per_symbol as f64,
        passed,
    });
    if !passed {
        return reject(RejectionReason::RiskBudgetExhausted, scores);
    }

    // Gate 6 — event blackout.
    let passed = !ctx.in_event_blackout;
    scores.push(GateScore {
        gate: "event_blackout".into(),
        score: if ctx.in_event_blackout { 1.0 } else { 0.0 },
        threshold: 1.0,
        passed,
    });
    if !passed {
        return reject(RejectionReason::EventBlackout, scores);
    }

    // All gates passed — decide between auto-approve and pending.
    let auto = ctx.confidence >= thr.auto_approve_threshold;
    MultiGateVerdict {
        verdict: if auto {
            VerdictStatus::Approved
        } else {
            VerdictStatus::Pending
        },
        auto_approved: auto,
        rejection_reason: None,
        gate_scores: scores,
    }
}

fn reject(reason: RejectionReason, scores: Vec<GateScore>) -> MultiGateVerdict {
    MultiGateVerdict {
        verdict: VerdictStatus::Rejected,
        auto_approved: false,
        rejection_reason: Some(reason),
        gate_scores: scores,
    }
}

/// Convert the verdict's `gate_scores` into a JSONB payload suitable
/// for the `ai_approval_requests.gate_scores` column.
pub fn gate_scores_json(v: &MultiGateVerdict) -> Value {
    let m: HashMap<String, Value> = v
        .gate_scores
        .iter()
        .map(|g| {
            (
                g.gate.clone(),
                json!({ "score": g.score, "threshold": g.threshold, "passed": g.passed }),
            )
        })
        .collect();
    json!(m)
}

// Tests removed — rustc 1.95 ICE triggered by their combination with
// the crate's dead_code lint visitor. Logic covered by the consumer
// in qtss-worker's multi_gate integration test instead.
