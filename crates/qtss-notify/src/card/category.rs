//! Asset category resolver.
//!
//! Maps `(exchange, symbol)` to a user-facing category like `KRİPTO`,
//! `FOREX`, `VADELİ`, `MEGA CAP`, etc. The resolver checks the DB
//! override table first, then falls back to venue-class heuristics.
//!
//! CLAUDE.md #1: the dispatch logic is a table of rules (trait impls)
//! — no scattered if/else. #2: tunable thresholds (mega_cap_top_n …)
//! come from `system_config`.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// The 13-row taxonomy from migration 0134. Kept in sync with
/// `asset_categories` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetCategory {
    MegaCap = 1,
    LargeCap = 2,
    MidCap = 3,
    Growth = 4,
    SmallCap = 5,
    Speculative = 6,
    MicroPenny = 7,
    Holding = 8,
    Endeks = 9,
    Emtia = 10,
    Forex = 11,
    Vadeli = 12,
    Kripto = 13,
}

impl AssetCategory {
    pub fn code(self) -> &'static str {
        match self {
            Self::MegaCap => "MEGA_CAP",
            Self::LargeCap => "LARGE_CAP",
            Self::MidCap => "MID_CAP",
            Self::Growth => "GROWTH",
            Self::SmallCap => "SMALL_CAP",
            Self::Speculative => "SPECULATIVE",
            Self::MicroPenny => "MICRO_PENNY",
            Self::Holding => "HOLDING",
            Self::Endeks => "ENDEKS",
            Self::Emtia => "EMTIA",
            Self::Forex => "FOREX",
            Self::Vadeli => "VADELI",
            Self::Kripto => "KRIPTO",
        }
    }

    pub fn label_tr(self) -> &'static str {
        match self {
            Self::MegaCap => "MEGA CAP",
            Self::LargeCap => "LARGE CAP",
            Self::MidCap => "MID CAP",
            Self::Growth => "GROWTH",
            Self::SmallCap => "SMALL CAP",
            Self::Speculative => "SPECULATIVE",
            Self::MicroPenny => "MICRO/PENNY",
            Self::Holding => "HOLDİNG",
            Self::Endeks => "ENDEKS",
            Self::Emtia => "EMTİA",
            Self::Forex => "FOREX",
            Self::Vadeli => "VADELİ",
            Self::Kripto => "KRİPTO",
        }
    }

    pub fn from_id(id: i16) -> Option<Self> {
        match id {
            1 => Some(Self::MegaCap),
            2 => Some(Self::LargeCap),
            3 => Some(Self::MidCap),
            4 => Some(Self::Growth),
            5 => Some(Self::SmallCap),
            6 => Some(Self::Speculative),
            7 => Some(Self::MicroPenny),
            8 => Some(Self::Holding),
            9 => Some(Self::Endeks),
            10 => Some(Self::Emtia),
            11 => Some(Self::Forex),
            12 => Some(Self::Vadeli),
            13 => Some(Self::Kripto),
            _ => None,
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "MEGA_CAP" => Some(Self::MegaCap),
            "LARGE_CAP" => Some(Self::LargeCap),
            "MID_CAP" => Some(Self::MidCap),
            "GROWTH" => Some(Self::Growth),
            "SMALL_CAP" => Some(Self::SmallCap),
            "SPECULATIVE" => Some(Self::Speculative),
            "MICRO_PENNY" => Some(Self::MicroPenny),
            "HOLDING" => Some(Self::Holding),
            "ENDEKS" => Some(Self::Endeks),
            "EMTIA" => Some(Self::Emtia),
            "FOREX" => Some(Self::Forex),
            "VADELI" => Some(Self::Vadeli),
            "KRIPTO" => Some(Self::Kripto),
            _ => None,
        }
    }
}

/// Context required for auto-classification when no DB override exists.
/// The worker populates these from the venue metadata it already
/// tracks (is this symbol a perpetual? what's its market-cap rank?).
#[derive(Debug, Clone)]
pub struct ResolveContext {
    pub exchange: String,
    pub symbol: String,
    /// "spot" | "perpetual" | "futures" | "forex" | "viop" | "index" | "commodity"
    pub venue_class: String,
    /// Crypto market-cap rank (1 = BTC). None for non-crypto.
    pub market_cap_rank: Option<i64>,
}

/// Resolver thresholds pulled from `system_config` by the caller.
#[derive(Debug, Clone, Copy)]
pub struct CategoryThresholds {
    pub crypto_mega_cap_top_n: i64,
    pub crypto_large_cap_top_n: i64,
    pub crypto_mid_cap_top_n: i64,
    pub crypto_small_cap_top_n: i64,
    pub crypto_futures_override: bool,
}

impl CategoryThresholds {
    pub const FALLBACK: Self = Self {
        crypto_mega_cap_top_n: 10,
        crypto_large_cap_top_n: 50,
        crypto_mid_cap_top_n: 200,
        crypto_small_cap_top_n: 1000,
        crypto_futures_override: true,
    };
}

/// Trait that implements one classification rule. CLAUDE.md #1 —
/// adding a new asset class = new impl, no if/else churn.
trait CategoryRule: Send + Sync {
    /// Return `Some(category)` if this rule claims the context.
    fn classify(
        &self,
        ctx: &ResolveContext,
        thresholds: &CategoryThresholds,
    ) -> Option<AssetCategory>;
    fn name(&self) -> &'static str;
}

// ── Rules (evaluated in declared order) ──────────────────────────────

struct ForexRule;
impl CategoryRule for ForexRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        _t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        matches!(ctx.venue_class.as_str(), "forex").then_some(AssetCategory::Forex)
    }
    fn name(&self) -> &'static str { "forex" }
}

struct ViopRule;
impl CategoryRule for ViopRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        _t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        matches!(ctx.venue_class.as_str(), "viop").then_some(AssetCategory::Vadeli)
    }
    fn name(&self) -> &'static str { "viop" }
}

struct IndexRule;
impl CategoryRule for IndexRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        _t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        matches!(ctx.venue_class.as_str(), "index").then_some(AssetCategory::Endeks)
    }
    fn name(&self) -> &'static str { "index" }
}

struct CommodityRule;
impl CategoryRule for CommodityRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        _t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        matches!(ctx.venue_class.as_str(), "commodity").then_some(AssetCategory::Emtia)
    }
    fn name(&self) -> &'static str { "commodity" }
}

/// Crypto futures / perpetual override — fires BEFORE the market-cap
/// tiering so BTCUSDT perpetual becomes VADELI rather than MEGA_CAP.
struct CryptoFuturesOverrideRule;
impl CategoryRule for CryptoFuturesOverrideRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        if !t.crypto_futures_override {
            return None;
        }
        match ctx.venue_class.as_str() {
            "perpetual" | "futures" => Some(AssetCategory::Vadeli),
            _ => None,
        }
    }
    fn name(&self) -> &'static str { "crypto_futures_override" }
}

/// Crypto market-cap tiering: top-N → MEGA / LARGE / MID / SMALL / MICRO.
struct CryptoMarketCapRule;
impl CategoryRule for CryptoMarketCapRule {
    fn classify(
        &self,
        ctx: &ResolveContext,
        t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        if ctx.venue_class != "spot" {
            return None;
        }
        // Unranked crypto → treat as MICRO/PENNY.
        let rank = ctx.market_cap_rank.unwrap_or(i64::MAX);
        let rules: [(i64, AssetCategory); 4] = [
            (t.crypto_mega_cap_top_n, AssetCategory::MegaCap),
            (t.crypto_large_cap_top_n, AssetCategory::LargeCap),
            (t.crypto_mid_cap_top_n, AssetCategory::MidCap),
            (t.crypto_small_cap_top_n, AssetCategory::SmallCap),
        ];
        Some(
            rules
                .iter()
                .find(|(cap, _)| rank <= *cap)
                .map(|(_, c)| *c)
                .unwrap_or(AssetCategory::MicroPenny),
        )
    }
    fn name(&self) -> &'static str { "crypto_market_cap" }
}

/// Fallback when no rule fires — conservative, visible-but-neutral tag.
struct FallbackSpeculativeRule;
impl CategoryRule for FallbackSpeculativeRule {
    fn classify(
        &self,
        _ctx: &ResolveContext,
        _t: &CategoryThresholds,
    ) -> Option<AssetCategory> {
        Some(AssetCategory::Speculative)
    }
    fn name(&self) -> &'static str { "fallback" }
}

fn rule_chain() -> Vec<Box<dyn CategoryRule>> {
    vec![
        Box::new(ForexRule),
        Box::new(ViopRule),
        Box::new(IndexRule),
        Box::new(CommodityRule),
        Box::new(CryptoFuturesOverrideRule),
        Box::new(CryptoMarketCapRule),
        Box::new(FallbackSpeculativeRule),
    ]
}

/// Classify using the in-memory rule chain. Public so unit tests can
/// exercise rules without a DB.
pub fn classify_by_rules(
    ctx: &ResolveContext,
    thresholds: &CategoryThresholds,
) -> (AssetCategory, &'static str) {
    for rule in rule_chain() {
        if let Some(cat) = rule.classify(ctx, thresholds) {
            return (cat, rule.name());
        }
    }
    // The FallbackSpeculativeRule always returns Some, so this is unreachable.
    (AssetCategory::Speculative, "fallback")
}

/// DB-first resolver: check `symbol_category_map` for a manual/auto
/// override; otherwise run the rule chain and optionally persist the
/// result with `source = 'auto'`.
pub async fn resolve(
    pool: &PgPool,
    ctx: &ResolveContext,
    thresholds: &CategoryThresholds,
    persist_auto: bool,
) -> AssetCategory {
    // DB lookup: explicit (manual or rule-engine) mapping takes priority.
    let db_row: Option<(i16,)> = sqlx::query_as::<_, (i16,)>(
        r#"SELECT category_id FROM symbol_category_map
           WHERE exchange = $1 AND symbol = $2 LIMIT 1"#,
    )
    .bind(&ctx.exchange)
    .bind(&ctx.symbol)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((id,)) = db_row {
        if let Some(cat) = AssetCategory::from_id(id) {
            return cat;
        }
    }

    // No override → classify by rules.
    let (cat, rule_name) = classify_by_rules(ctx, thresholds);

    if persist_auto {
        // Best-effort write; resolver must not fail on persist errors.
        let _ = sqlx::query(
            r#"INSERT INTO symbol_category_map (exchange, symbol, category_id, source, updated_at)
               VALUES ($1, $2, $3, $4, NOW())
               ON CONFLICT (exchange, symbol)
                 DO UPDATE SET category_id = EXCLUDED.category_id,
                               source     = EXCLUDED.source,
                               updated_at = NOW()
                 WHERE symbol_category_map.source <> 'manual'"#,
        )
        .bind(&ctx.exchange)
        .bind(&ctx.symbol)
        .bind(cat as i16)
        .bind(format!("auto:{rule_name}"))
        .execute(pool)
        .await;
    }

    cat
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(venue: &str, rank: Option<i64>) -> ResolveContext {
        ResolveContext {
            exchange: "binance".into(),
            symbol: "BTCUSDT".into(),
            venue_class: venue.into(),
            market_cap_rank: rank,
        }
    }

    #[test]
    fn spot_btc_is_mega_cap() {
        let (cat, name) = classify_by_rules(
            &ctx("spot", Some(1)),
            &CategoryThresholds::FALLBACK,
        );
        assert_eq!(cat, AssetCategory::MegaCap);
        assert_eq!(name, "crypto_market_cap");
    }

    #[test]
    fn spot_rank_75_is_mid_cap() {
        let (cat, _) = classify_by_rules(
            &ctx("spot", Some(75)),
            &CategoryThresholds::FALLBACK,
        );
        assert_eq!(cat, AssetCategory::MidCap);
    }

    #[test]
    fn perpetual_overrides_to_vadeli() {
        let (cat, name) = classify_by_rules(
            &ctx("perpetual", Some(1)),
            &CategoryThresholds::FALLBACK,
        );
        assert_eq!(cat, AssetCategory::Vadeli);
        assert_eq!(name, "crypto_futures_override");
    }

    #[test]
    fn perpetual_override_disabled_falls_back_to_market_cap() {
        let t = CategoryThresholds {
            crypto_futures_override: false,
            ..CategoryThresholds::FALLBACK
        };
        // When override disabled AND venue_class != "spot", market_cap
        // rule also bails → falls through to Speculative.
        let (cat, name) = classify_by_rules(&ctx("perpetual", Some(1)), &t);
        assert_eq!(cat, AssetCategory::Speculative);
        assert_eq!(name, "fallback");
    }

    #[test]
    fn forex_detected() {
        let (cat, _) = classify_by_rules(
            &ctx("forex", None),
            &CategoryThresholds::FALLBACK,
        );
        assert_eq!(cat, AssetCategory::Forex);
    }

    #[test]
    fn unranked_crypto_is_micro_penny() {
        let (cat, _) = classify_by_rules(
            &ctx("spot", None),
            &CategoryThresholds::FALLBACK,
        );
        assert_eq!(cat, AssetCategory::MicroPenny);
    }

    #[test]
    fn labels_and_codes_roundtrip() {
        for c in [
            AssetCategory::MegaCap,
            AssetCategory::Vadeli,
            AssetCategory::Kripto,
            AssetCategory::Forex,
        ] {
            assert_eq!(AssetCategory::from_code(c.code()), Some(c));
        }
    }
}
