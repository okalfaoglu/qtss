//! PublicCard — the channel-agnostic user-facing view of a setup.
//!
//! All renderers (Telegram, X, digest, GUI preview) consume this
//! struct. The builder takes a `SetupSnapshot` (the data we already
//! have in `qtss_v2_setups` + the AI score) and produces a PublicCard
//! with the tier badge + asset category already resolved.
//!
//! CLAUDE.md #3: this crate knows nothing about orders, strategy,
//! or detection internals — only what a user needs to see.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::category::{self, AssetCategory, CategoryThresholds, ResolveContext};
use super::tier::{TierBadge, TierThresholds};

/// Setup direction — LONG or SHORT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupDirection {
    Long,
    Short,
}

impl SetupDirection {
    pub fn label_tr(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
        }
    }
}

/// Faz 9.7.8 — Optional AI rationale carried on a new-setup broadcast.
///
/// Populated by the caller from `qtss_ml_predictions` (score-level
/// signal) and the AI decision layer (action + free-form reasoning).
/// Mirror of the AI fields already carried on `LifecycleContext` so
/// the initial broadcast and subsequent lifecycle updates read
/// consistently across channels.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiBrief {
    /// e.g. "Enter", "Watch", "Skip" — short verb from the decision layer.
    pub action: Option<String>,
    /// 1-2 sentence human rationale (Turkish when the LLM tiebreaker
    /// is enabled). Rendered as-is after HTML escaping.
    pub reasoning: Option<String>,
    /// [0, 1] — confidence reported by the decision layer.
    pub confidence: Option<f64>,
    /// Top feature names driving the score (capped at 3 by the caller).
    #[serde(default)]
    pub top_features: Vec<String>,
}

impl AiBrief {
    pub fn is_empty(&self) -> bool {
        self.action.is_none()
            && self.reasoning.is_none()
            && self.confidence.is_none()
            && self.top_features.is_empty()
    }
}

/// Input to the card builder. Callers populate this from `qtss_v2_setups`
/// joined with `qtss_ml_predictions`.
#[derive(Debug, Clone)]
pub struct SetupSnapshot {
    pub setup_id: uuid::Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub venue_class: String,             // for category resolver
    pub market_cap_rank: Option<i64>,    // for category resolver
    pub direction: SetupDirection,
    pub pattern_family: String,          // "wyckoff" | "harmonic" | ...
    pub pattern_subkind: Option<String>, // "spring" | "bat" | ...
    pub ai_score: f64,                   // [0,1]
    pub entry_price: Decimal,
    pub stop_price: Decimal,
    pub tp1_price: Option<Decimal>,
    pub tp2_price: Option<Decimal>,
    pub tp3_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub created_at: DateTime<Utc>,
    /// Faz 9.7.8 — optional AI rationale. Callers pass `None` when the
    /// decision layer is offline or below the confidence gate.
    #[doc(hidden)]
    pub ai_brief: Option<AiBrief>,
}

/// Fully rendered public card — channel-independent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCard {
    pub setup_id: uuid::Uuid,
    pub symbol: String,
    pub timeframe: String,
    pub category: AssetCategory,
    pub category_label: String,     // pre-localised for ease of render
    pub direction: SetupDirection,
    pub pattern_label: String,      // "Wyckoff · Spring"
    pub tier: TierBadge,
    // Price section.
    pub current_price: Option<Decimal>,
    pub current_change_pct: Option<f64>,   // vs entry (live) or none
    pub entry_price: Decimal,
    pub stop_price: Decimal,
    pub stop_pct: f64,                      // signed % from entry
    /// Ordered TP prices + associated % from entry.
    pub targets: Vec<TargetPoint>,
    /// Primary R:R derived from (entry - stop) vs (first target - entry).
    pub risk_reward: Option<f64>,
    pub created_at: DateTime<Utc>,
    /// Faz 9.7.8 — AI rationale threaded from the snapshot. `None` when
    /// the caller didn't populate one (degraded mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_brief: Option<AiBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetPoint {
    /// Index: 1 for TP1, 2 for TP2, 3 for TP3.
    pub index: u8,
    pub price: Decimal,
    pub pct: f64,
}

impl PublicCard {
    /// Build a card from the snapshot. Runs DB-backed category lookup
    /// and config-backed tier thresholds. Errors are soft — falls back
    /// to safe defaults so a render never fails because of config.
    pub async fn build(
        pool: &PgPool,
        snapshot: SetupSnapshot,
        tier_thresholds: TierThresholds,
        category_thresholds: CategoryThresholds,
    ) -> Self {
        let resolve_ctx = ResolveContext {
            exchange: snapshot.exchange.clone(),
            symbol: snapshot.symbol.clone(),
            venue_class: snapshot.venue_class.clone(),
            market_cap_rank: snapshot.market_cap_rank,
        };
        let category = category::resolve(pool, &resolve_ctx, &category_thresholds, true).await;
        Self::build_from_parts(snapshot, tier_thresholds, category)
    }

    /// Pure variant for unit tests and GUI preview — no DB access.
    pub fn build_from_parts(
        snapshot: SetupSnapshot,
        tier_thresholds: TierThresholds,
        category: AssetCategory,
    ) -> Self {
        let tier = TierBadge::build(snapshot.ai_score, &tier_thresholds);
        let entry_f = to_f64(snapshot.entry_price);
        let stop_f = to_f64(snapshot.stop_price);
        let stop_pct = pct_from_entry(entry_f, stop_f);
        let targets = [snapshot.tp1_price, snapshot.tp2_price, snapshot.tp3_price]
            .iter()
            .enumerate()
            .filter_map(|(i, maybe)| maybe.map(|p| (i as u8 + 1, p)))
            .map(|(idx, price)| TargetPoint {
                index: idx,
                price,
                pct: pct_from_entry(entry_f, to_f64(price)),
            })
            .collect::<Vec<_>>();
        let risk_reward = compute_risk_reward(
            snapshot.direction,
            entry_f,
            stop_f,
            targets.first().map(|t| to_f64(t.price)),
        );
        let current_change_pct = snapshot
            .current_price
            .map(|cp| pct_from_entry(entry_f, to_f64(cp)));
        let pattern_label = format_pattern(&snapshot.pattern_family, snapshot.pattern_subkind.as_deref());
        let ai_brief = snapshot
            .ai_brief
            .filter(|b| !b.is_empty());
        Self {
            setup_id: snapshot.setup_id,
            symbol: snapshot.symbol,
            timeframe: snapshot.timeframe,
            category,
            category_label: category.label_tr().to_string(),
            direction: snapshot.direction,
            pattern_label,
            tier,
            current_price: snapshot.current_price,
            current_change_pct,
            entry_price: snapshot.entry_price,
            stop_price: snapshot.stop_price,
            stop_pct,
            targets,
            risk_reward,
            created_at: snapshot.created_at,
            ai_brief,
        }
    }

    /// Direction-adjusted sign for "profit on this move" calculations.
    pub fn dir_sign(&self) -> f64 {
        match self.direction {
            SetupDirection::Long => 1.0,
            SetupDirection::Short => -1.0,
        }
    }
}

fn to_f64(d: Decimal) -> f64 {
    // Decimal → f64 never fails for representable money amounts.
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

fn pct_from_entry(entry: f64, other: f64) -> f64 {
    if entry.abs() < 1e-12 {
        return 0.0;
    }
    (other - entry) / entry * 100.0
}

fn compute_risk_reward(
    direction: SetupDirection,
    entry: f64,
    stop: f64,
    tp1: Option<f64>,
) -> Option<f64> {
    let tp1 = tp1?;
    let (risk, reward) = match direction {
        SetupDirection::Long => (entry - stop, tp1 - entry),
        SetupDirection::Short => (stop - entry, entry - tp1),
    };
    if risk <= 0.0 || reward <= 0.0 {
        return None;
    }
    Some(reward / risk)
}

fn format_pattern(family: &str, subkind: Option<&str>) -> String {
    // Simple title-case without external deps; avoids dragging `heck`.
    let fam = title_case(family);
    match subkind {
        Some(s) if !s.is_empty() => format!("{fam} · {}", title_case(s)),
        _ => fam,
    }
}

fn title_case(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn snap() -> SetupSnapshot {
        SetupSnapshot {
            setup_id: uuid::Uuid::new_v4(),
            exchange: "binance".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            venue_class: "spot".into(),
            market_cap_rank: Some(1),
            direction: SetupDirection::Long,
            pattern_family: "wyckoff".into(),
            pattern_subkind: Some("spring".into()),
            ai_score: 0.78,
            entry_price: dec!(82_400),
            stop_price: dec!(81_100),
            tp1_price: Some(dec!(85_200)),
            tp2_price: Some(dec!(87_500)),
            tp3_price: None,
            current_price: Some(dec!(82_950)),
            created_at: Utc::now(),
            ai_brief: None,
        }
    }

    #[test]
    fn long_card_computes_signed_pcts() {
        let c = PublicCard::build_from_parts(
            snap(),
            TierThresholds::FALLBACK,
            AssetCategory::MegaCap,
        );
        // Stop below entry on LONG → negative %.
        assert!(c.stop_pct < 0.0);
        // Targets above entry on LONG → positive %.
        assert!(c.targets.iter().all(|t| t.pct > 0.0));
        assert_eq!(c.targets.len(), 2);
        // R:R = (85200-82400)/(82400-81100) ≈ 2.15
        let rr = c.risk_reward.unwrap();
        assert!((rr - 2.153).abs() < 0.01);
        assert_eq!(c.category, AssetCategory::MegaCap);
        assert_eq!(c.tier.out_of_ten, 9);
    }

    #[test]
    fn short_card_inverts_rr() {
        let mut s = snap();
        s.direction = SetupDirection::Short;
        s.entry_price = dec!(82_400);
        s.stop_price = dec!(83_500);    // stop ABOVE entry on SHORT
        s.tp1_price = Some(dec!(80_000)); // target BELOW entry
        s.tp2_price = None;
        s.tp3_price = None;
        let c = PublicCard::build_from_parts(s, TierThresholds::FALLBACK, AssetCategory::Kripto);
        // R:R = (82400-80000)/(83500-82400) ≈ 2.18
        let rr = c.risk_reward.unwrap();
        assert!((rr - 2.181).abs() < 0.01);
        // Stop % positive (above entry) but still "bad for SHORT".
        assert!(c.stop_pct > 0.0);
    }

    #[test]
    fn pattern_label_title_cases() {
        let c = PublicCard::build_from_parts(
            snap(),
            TierThresholds::FALLBACK,
            AssetCategory::MegaCap,
        );
        assert_eq!(c.pattern_label, "Wyckoff · Spring");
    }

    #[test]
    fn no_tp_yields_no_rr() {
        let mut s = snap();
        s.tp1_price = None;
        s.tp2_price = None;
        s.tp3_price = None;
        let c = PublicCard::build_from_parts(s, TierThresholds::FALLBACK, AssetCategory::MegaCap);
        assert!(c.risk_reward.is_none());
        assert!(c.targets.is_empty());
    }
}
