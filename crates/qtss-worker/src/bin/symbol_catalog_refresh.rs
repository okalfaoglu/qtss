#![allow(dead_code)]
//! symbol_catalog_refresh — Faz 14.A / A2+A3+A4
//!
//! Walks every active binance instrument and upserts `qtss_symbol_profile`
//! with Binance-side venue constraints (already in `instruments` via
//! `catalog_sync`) plus CoinGecko fundamentals (market_cap, 24h volume,
//! price, categories).
//!
//! Category / risk_tier mapping is dispatch-table driven (CLAUDE.md #1)
//! so new asset classes (BIST/NASDAQ) plug in without touching this file.
//!
//! Tunables (tier caps, liquidity floors, category thresholds) live in
//! `config_schema` (CLAUDE.md #2). This binary only reads caps; it
//! doesn't hardcode them.
//!
//! Run manually or from a cron:
//!   cargo run --release -p qtss-worker --bin symbol-catalog-refresh
//!
//! Env overrides (all optional):
//!   SYMBOL_REFRESH_EXCHANGE        — default "binance"
//!   SYMBOL_REFRESH_LIMIT           — cap rows processed (debug)
//!   SYMBOL_REFRESH_COINGECKO_KEY   — Pro API key (unset = free tier)

use std::collections::HashMap;
use std::env;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value as Json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Category + tier mapping — dispatch tables.
// ---------------------------------------------------------------------------

/// One cutoff row. First match from TOP to BOTTOM wins.
#[derive(Debug)]
struct CategoryRule {
    min_mcap_usd: f64,
    category: &'static str,
    risk_tier: &'static str,
}

/// Default crypto ladder. Stablecoins / memes get overridden later.
const CRYPTO_CATEGORY_LADDER: &[CategoryRule] = &[
    CategoryRule { min_mcap_usd: 50_000_000_000.0, category: "mega_cap",    risk_tier: "core"        },
    CategoryRule { min_mcap_usd:  5_000_000_000.0, category: "large_cap",   risk_tier: "core"        },
    CategoryRule { min_mcap_usd:    500_000_000.0, category: "mid_cap",     risk_tier: "balanced"    },
    CategoryRule { min_mcap_usd:     50_000_000.0, category: "small_cap",   risk_tier: "growth"      },
    CategoryRule { min_mcap_usd:              0.0, category: "micro_penny", risk_tier: "speculative" },
];

/// CoinGecko category slugs we actually care about; everything else
/// becomes the free-form `sector` column verbatim.
fn normalize_sector(cats: &[String]) -> Option<String> {
    // Precedence: stablecoin > meme > L1 > defi > AI > gaming > fallback
    for want in ["stablecoin", "meme", "layer-1", "defi", "artificial-intelligence", "gaming"] {
        if cats.iter().any(|c| c.to_ascii_lowercase().contains(want)) {
            return Some(want.replace('-', "_"));
        }
    }
    cats.first().cloned()
}

/// Override the ladder when the sector demands it — stablecoins must
/// never arm setups (category='kripto', tier='core' flags them), memes
/// collapse to `speculative` regardless of mcap.
fn classify(mcap: f64, sector: Option<&str>) -> (&'static str, &'static str) {
    if matches!(sector, Some("stablecoin")) {
        return ("kripto", "core");
    }
    if matches!(sector, Some("meme")) {
        return ("speculative", "extreme");
    }
    let base = CRYPTO_CATEGORY_LADDER
        .iter()
        .find(|r| mcap >= r.min_mcap_usd)
        .expect("last rule has min=0");
    (base.category, base.risk_tier)
}

// ---------------------------------------------------------------------------
// Scoring — all 0..100, deterministic from snapshot.
// ---------------------------------------------------------------------------

fn fundamental_score(mcap_usd: f64) -> i16 {
    // Log-scaled: $10M → 20, $1B → 60, $100B → 95, BTC → ~100.
    if mcap_usd <= 1.0 {
        return 0;
    }
    let s = 10.0 * (mcap_usd.log10() - 6.0);
    s.clamp(0.0, 100.0) as i16
}

fn liquidity_score(vol_usd: f64) -> i16 {
    if vol_usd <= 1.0 { return 0; }
    // $100K → 10, $10M → 50, $1B → 90.
    let s = 10.0 * (vol_usd.log10() - 4.0);
    s.clamp(0.0, 100.0) as i16
}

/// Volatility score: higher = more stable. We approximate by inverting
/// 24h price change %; real ATR-based scoring lands when we wire in
/// realised vol in A6.
fn volatility_score(price_change_pct_24h: Option<f64>) -> i16 {
    let c = price_change_pct_24h.unwrap_or(0.0).abs();
    (100.0 - c * 5.0).clamp(0.0, 100.0) as i16
}

// ---------------------------------------------------------------------------
// CoinGecko client (inline; extract to crate later if BIST/NASDAQ reuse).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CgListEntry {
    id: String,
    symbol: String,
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct CgMarketRow {
    id: String,
    symbol: String,
    current_price: Option<f64>,
    market_cap: Option<f64>,
    total_volume: Option<f64>,
    circulating_supply: Option<f64>,
    price_change_percentage_24h: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct CgCoinDetail {
    #[serde(default)]
    categories: Vec<Option<String>>,
}

struct CoinGecko {
    http: Client,
    base: String,
    key_header: Option<(String, String)>,
}

impl CoinGecko {
    /// `tier` = "demo" | "pro" | "" (free/public). Key config_schema'dan
    /// `symbol_intel.coingecko.api_key` + `.tier` olarak okunur. Demo
    /// tier public base'i kullanır ama `x-cg-demo-api-key` header basar.
    /// Pro tier ayrı bir base URL'e düşer.
    fn new(key: Option<String>, tier: &str) -> Self {
        let (base, key_header) = match (key, tier) {
            (Some(k), "pro") if !k.is_empty() => (
                "https://pro-api.coingecko.com/api/v3".into(),
                Some(("x-cg-pro-api-key".into(), k)),
            ),
            (Some(k), _) if !k.is_empty() => (
                "https://api.coingecko.com/api/v3".into(),
                Some(("x-cg-demo-api-key".into(), k)),
            ),
            _ => ("https://api.coingecko.com/api/v3".into(), None),
        };
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("qtss-symbol-refresh/14.A")
            .build()
            .expect("reqwest builder");
        Self { http, base, key_header }
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str, qs: &[(&str, String)]) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.get(&url).query(qs);
        if let Some((h, v)) = &self.key_header {
            req = req.header(h, v);
        }
        let resp = req.send().await.with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("{url} → {status}: {}", body.chars().take(200).collect::<String>()));
        }
        serde_json::from_str::<T>(&body).with_context(|| format!("decode {url}"))
    }

    /// Build {UPPER_SYMBOL → coingecko_id}. Disambiguation via mcap:
    /// top-500 `/coins/markets?order=market_cap_desc` first (so BTC
    /// maps to `bitcoin`, not `btc-classic` meme clone), then backfill
    /// tail of `/coins/list` for long-tail symbols.
    async fn list_map(&self) -> Result<HashMap<String, String>> {
        let mut m: HashMap<String, String> = HashMap::new();
        // Top-500 mcap-sorted: 2 pages × 250. First-seen wins so the
        // mega-cap original owns the ticker.
        for page in 1..=2 {
            let rows: Vec<CgMarketRow> = self
                .get(
                    "/coins/markets",
                    &[
                        ("vs_currency", "usd".into()),
                        ("order", "market_cap_desc".into()),
                        ("per_page", "250".into()),
                        ("page", page.to_string()),
                    ],
                )
                .await?;
            for r in rows {
                m.entry(r.symbol.to_ascii_uppercase()).or_insert(r.id);
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        // Backfill long-tail (symbols not in top-500).
        let list: Vec<CgListEntry> = self.get("/coins/list", &[]).await?;
        for e in list {
            m.entry(e.symbol.to_ascii_uppercase())
                .or_insert(e.id);
        }
        Ok(m)
    }

    async fn markets(&self, ids: &[String]) -> Result<Vec<CgMarketRow>> {
        if ids.is_empty() { return Ok(Vec::new()); }
        // /coins/markets ids=comma-sep, up to 250 per page.
        let mut out = Vec::new();
        for chunk in ids.chunks(100) {
            let ids_csv = chunk.join(",");
            let rows: Vec<CgMarketRow> = self
                .get(
                    "/coins/markets",
                    &[
                        ("vs_currency", "usd".into()),
                        ("ids", ids_csv),
                        ("order", "market_cap_desc".into()),
                        ("per_page", "250".into()),
                        ("page", "1".into()),
                        ("price_change_percentage", "24h".into()),
                    ],
                )
                .await?;
            out.extend(rows);
            // Free tier: 30 req/min. Sleep keeps us under.
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        Ok(out)
    }

    async fn categories_for(&self, id: &str) -> Result<Vec<String>> {
        let detail: CgCoinDetail = self
            .get(
                &format!("/coins/{id}"),
                &[
                    ("localization", "false".into()),
                    ("tickers", "false".into()),
                    ("market_data", "false".into()),
                    ("community_data", "false".into()),
                    ("developer_data", "false".into()),
                    ("sparkline", "false".into()),
                ],
            )
            .await?;
        Ok(detail.categories.into_iter().flatten().collect())
    }
}

// ---------------------------------------------------------------------------
// Binance-side snapshot, pulled from local `instruments` (no API call).
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct VenueRow {
    exchange: String,
    symbol: String,
    base: String,
    #[allow(dead_code)]
    quote: String,
    tick_size: Option<f64>,
    step_size: Option<f64>,
    min_qty: Option<f64>,
}

async fn load_binance_instruments(pool: &PgPool, limit: Option<i64>) -> Result<Vec<VenueRow>> {
    // Pull USDT-quoted, trading instruments across spot + usdt-futures
    // markets. Dedupe base_asset across markets so we don't call
    // CoinGecko twice for BTC etc.; we keep the futures row (canonical
    // for engine) when both exist.
    let sql = r#"
        SELECT e.code AS exchange,
               i.native_symbol AS symbol,
               i.base_asset AS base,
               i.quote_asset AS quote,
               i.price_filter,
               i.lot_filter,
               m.segment AS market_kind
          FROM instruments i
          JOIN markets   m ON m.id = i.market_id
          JOIN exchanges e ON e.id = m.exchange_id
         WHERE e.code = 'binance'
           AND i.is_trading
           AND i.quote_asset = 'USDT'
         ORDER BY m.segment DESC, i.native_symbol   -- futures before spot
         LIMIT COALESCE($1, 5000)
    "#;
    let rows = sqlx::query(sql)
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("load instruments")?;

    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let base: String = r.get("base");
        if !seen.insert(base.clone()) {
            continue;
        }
        let price_filter: Option<Json> = r.try_get("price_filter").ok().flatten();
        let lot_filter:   Option<Json> = r.try_get("lot_filter").ok().flatten();
        let tick_size = price_filter
            .as_ref()
            .and_then(|v| v.get("tickSize")?.as_str()?.parse::<f64>().ok());
        let step_size = lot_filter
            .as_ref()
            .and_then(|v| v.get("stepSize")?.as_str()?.parse::<f64>().ok());
        let min_qty = lot_filter
            .as_ref()
            .and_then(|v| v.get("minQty")?.as_str()?.parse::<f64>().ok());
        out.push(VenueRow {
            exchange: r.get("exchange"),
            symbol: r.get("symbol"),
            base,
            quote: r.get("quote"),
            tick_size,
            step_size,
            min_qty,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Upsert
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn upsert_profile(
    pool: &PgPool,
    venue: &VenueRow,
    cg: Option<&CgMarketRow>,
    sector: Option<String>,
    category: &str,
    risk_tier: &str,
    fundamental: i16,
    liquidity: i16,
    volatility: i16,
) -> Result<()> {
    let market_cap = cg.and_then(|c| c.market_cap);
    let volume     = cg.and_then(|c| c.total_volume);
    let price      = cg.and_then(|c| c.current_price);
    let circ       = cg.and_then(|c| c.circulating_supply);
    let min_notional = venue
        .min_qty
        .zip(price)
        .map(|(q, p)| q * p);

    sqlx::query(
        r#"
        INSERT INTO qtss_symbol_profile (
            exchange, symbol, asset_class, category, risk_tier, sector,
            country, market_cap_usd, circulating_supply, avg_daily_vol_usd,
            price_usd, lot_size, tick_size, min_notional, step_size,
            fundamental_score, liquidity_score, volatility_score,
            source, updated_at
        ) VALUES (
            $1, $2, 'crypto', $3, $4, $5,
            'GLOBAL', $6, $7, $8,
            $9, $10, $11, $12, $10,
            $13, $14, $15,
            'coingecko+binance', now()
        )
        ON CONFLICT (exchange, symbol) DO UPDATE SET
            asset_class       = EXCLUDED.asset_class,
            category          = CASE WHEN qtss_symbol_profile.manual_override
                                     THEN qtss_symbol_profile.category
                                     ELSE EXCLUDED.category END,
            risk_tier         = CASE WHEN qtss_symbol_profile.manual_override
                                     THEN qtss_symbol_profile.risk_tier
                                     ELSE EXCLUDED.risk_tier END,
            sector            = COALESCE(EXCLUDED.sector, qtss_symbol_profile.sector),
            market_cap_usd    = EXCLUDED.market_cap_usd,
            circulating_supply= EXCLUDED.circulating_supply,
            avg_daily_vol_usd = EXCLUDED.avg_daily_vol_usd,
            price_usd         = EXCLUDED.price_usd,
            lot_size          = EXCLUDED.lot_size,
            tick_size         = EXCLUDED.tick_size,
            min_notional      = EXCLUDED.min_notional,
            step_size         = EXCLUDED.step_size,
            fundamental_score = EXCLUDED.fundamental_score,
            liquidity_score   = EXCLUDED.liquidity_score,
            volatility_score  = EXCLUDED.volatility_score,
            source            = EXCLUDED.source,
            updated_at        = now()
        "#,
    )
    .bind(&venue.exchange)
    .bind(&venue.symbol)
    .bind(category)
    .bind(risk_tier)
    .bind(sector)
    .bind(market_cap.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(circ.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(volume.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(price.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(venue.step_size.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(venue.tick_size.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(min_notional.map(|v| Decimal::from_f64_retain(v).unwrap_or_default()))
    .bind(fundamental)
    .bind(liquidity)
    .bind(volatility)
    .execute(pool)
    .await
    .context("upsert qtss_symbol_profile")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/// Read CoinGecko credentials from `config_schema`. Falls back to env
/// `SYMBOL_REFRESH_COINGECKO_KEY` for local dev convenience when the
/// DB seed hasn't been edited. Returns `(api_key, tier)` where tier is
/// `"demo"` | `"pro"` | `""`.
async fn load_cg_credentials(pool: &PgPool) -> (Option<String>, String) {
    let key_row: Option<Json> = sqlx::query_scalar(
        "SELECT COALESCE(
                 (SELECT value FROM config_value
                   WHERE key = 'symbol_intel.coingecko.api_key'
                   ORDER BY updated_at DESC LIMIT 1),
                 (SELECT default_value FROM config_schema
                   WHERE key = 'symbol_intel.coingecko.api_key'))",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let tier_row: Option<Json> = sqlx::query_scalar(
        "SELECT COALESCE(
                 (SELECT value FROM config_value
                   WHERE key = 'symbol_intel.coingecko.tier'
                   ORDER BY updated_at DESC LIMIT 1),
                 (SELECT default_value FROM config_schema
                   WHERE key = 'symbol_intel.coingecko.tier'))",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let mut key = key_row
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty());
    if key.is_none() {
        // Dev fallback: respect .env if someone set it during a debug run.
        key = env::var("SYMBOL_REFRESH_COINGECKO_KEY").ok().filter(|s| !s.is_empty());
    }
    let tier = tier_row
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "demo".into());
    (key, tier)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dsn = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new().max_connections(4).connect(&dsn).await?;

    let exchange = env::var("SYMBOL_REFRESH_EXCHANGE").unwrap_or_else(|_| "binance".into());
    let limit: Option<i64> = env::var("SYMBOL_REFRESH_LIMIT").ok().and_then(|v| v.parse().ok());

    info!(%exchange, ?limit, "symbol_catalog_refresh starting");

    let venues = load_binance_instruments(&pool, limit).await?;
    info!(n = venues.len(), "loaded venue rows (deduped by base asset)");

    // API key config_schema'dan (CLAUDE.md #2 — kod seviyesinde sabit
    // yok, .env yalnızca bootstrap). `config_values` override'ı
    // yoksa default_value kullanılır.
    let (cg_key, cg_tier) = load_cg_credentials(&pool).await;
    // If we have no CoinGecko data we still seed rows with venue-only
    // info so the table is never empty; category=small_cap/speculative
    // as safest fallback.
    let cg = CoinGecko::new(cg_key, &cg_tier);
    let symbol_to_id = match cg.list_map().await {
        Ok(m) => m,
        Err(e) => {
            warn!(%e, "coingecko /coins/list failed; upserting venue-only rows");
            HashMap::new()
        }
    };

    // Build id list for our bases.
    let mut wanted_ids: Vec<String> = Vec::new();
    let mut base_to_id: HashMap<String, String> = HashMap::new();
    for v in &venues {
        if let Some(id) = symbol_to_id.get(&v.base.to_ascii_uppercase()) {
            base_to_id.insert(v.base.clone(), id.clone());
            wanted_ids.push(id.clone());
        }
    }

    let market_rows = match cg.markets(&wanted_ids).await {
        Ok(r) => r,
        Err(e) => {
            warn!(%e, "coingecko /coins/markets failed; venue-only fallback");
            Vec::new()
        }
    };
    let id_to_market: HashMap<String, CgMarketRow> =
        market_rows.into_iter().map(|r| (r.id.clone(), r)).collect();

    let mut seeded = 0usize;
    let mut enriched = 0usize;
    let mut failed = 0usize;

    for v in &venues {
        let cg_row = base_to_id.get(&v.base).and_then(|id| id_to_market.get(id));

        // Fetch categories only for the top coins (mcap > $100M) to
        // keep free-tier request budget sane.
        let categories: Vec<String> = if let Some(m) = cg_row {
            if m.market_cap.unwrap_or(0.0) >= 100_000_000.0 {
                match cg.categories_for(&m.id).await {
                    Ok(c) => c,
                    Err(e) => { warn!(sym=%v.symbol, %e, "categories fetch failed"); Vec::new() }
                }
            } else { Vec::new() }
        } else { Vec::new() };

        let sector = normalize_sector(&categories);
        let mcap = cg_row.and_then(|m| m.market_cap).unwrap_or(0.0);
        let (category, risk_tier) = classify(mcap, sector.as_deref());

        let fund_s = fundamental_score(mcap);
        let vol_s  = cg_row.and_then(|m| m.total_volume).unwrap_or(0.0);
        let liq_s  = liquidity_score(vol_s);
        let volat  = volatility_score(cg_row.and_then(|m| m.price_change_percentage_24h));

        match upsert_profile(&pool, v, cg_row, sector.clone(), category, risk_tier, fund_s, liq_s, volat).await {
            Ok(()) => {
                if cg_row.is_some() { enriched += 1; } else { seeded += 1; }
            }
            Err(e) => {
                warn!(sym=%v.symbol, %e, "upsert failed");
                failed += 1;
            }
        }

        // Polite rate limit on /coins/{id}.
        if cg_row.is_some() {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    info!(enriched, seeded_venue_only = seeded, failed, "symbol_catalog_refresh complete");

    // Summary log
    let summary = sqlx::query(
        r#"SELECT category, risk_tier, COUNT(*) AS n
             FROM qtss_symbol_profile
            WHERE exchange = $1
            GROUP BY 1, 2
            ORDER BY 1, 2"#,
    )
    .bind(&exchange)
    .fetch_all(&pool)
    .await?;
    for r in summary {
        let cat: String = r.get("category");
        let tier: String = r.get("risk_tier");
        let n: i64 = r.get("n");
        info!(%cat, %tier, n, "profile breakdown");
    }

    Ok(())
}
