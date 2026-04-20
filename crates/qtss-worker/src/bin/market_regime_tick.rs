//! market_regime_tick — Faz 14.A6.
//!
//! Günlük borsa-geneli rejim snapshot'ı. Şimdilik crypto (binance)
//! için `market_bars` 1d serisinden:
//!   * breadth_pct  = 20-gün üstünde kapanış yapan sembol %'si
//!   * momentum_20d = sembol başına (close/close_20d_ago - 1) medyan
//!   * volatility_index = 20-gün ort. ATR% medyan
//!   * regime sınıflandırması:
//!         breadth > 70  &&  momentum > +3%  → risk_on
//!         breadth > 50                      → neutral
//!         breadth > 25                      → risk_off
//!         else                              → panic
//! Eşikler `market_regime.*` anahtarları altında config'den okunur.
//!
//! Sadece `sector='*'` (borsa geneli) satır üretir. Sektör-özel
//! kırılım ayrı iterasyonda eklenecek. CLAUDE.md #2 — tüm eşikler
//! config'ten geliyor; hardcoded varsayılan sadece fall-back.

#![allow(clippy::too_many_arguments)]

use chrono::Utc;
use qtss_config::{ConfigStore, PgConfigStore, ResolveCtx};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db = std::env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new().max_connections(4).connect(&db).await?;
    let cfg = PgConfigStore::new(pool.clone());

    let exchange = std::env::var("REGIME_EXCHANGE").unwrap_or_else(|_| "binance".into());
    tick(&pool, &cfg, &exchange).await
}

async fn tick(pool: &PgPool, cfg: &PgConfigStore, exchange: &str) -> anyhow::Result<()> {
    let ctx = ResolveCtx::default().with_venue(exchange);

    // Eşikleri config'ten al; yoksa fall-back.
    let thresh: BTreeMap<String, f64> = cfg
        .get("market_regime.thresholds", &ctx)
        .await
        .unwrap_or_else(|_| {
            BTreeMap::from([
                ("risk_on_breadth".into(), 70.0),
                ("risk_on_momentum_pct".into(), 3.0),
                ("neutral_breadth".into(), 50.0),
                ("risk_off_breadth".into(), 25.0),
            ])
        });

    // Her sembol için 20-gün momentumu ve 20g ATR% — tek sorguda.
    let rows = sqlx::query(
        r#"
        WITH win AS (
          SELECT symbol,
                 open_time,
                 close,
                 (high - low) / NULLIF(close, 0) * 100.0 AS rng_pct,
                 ROW_NUMBER() OVER (PARTITION BY symbol ORDER BY open_time DESC) AS rn
            FROM market_bars
           WHERE exchange = $1
             AND interval = '1d'
             AND open_time > now() - interval '30 days'
        )
        SELECT symbol,
               MAX(close) FILTER (WHERE rn = 1)    AS close_now,
               MAX(close) FILTER (WHERE rn = 20)   AS close_20,
               AVG(rng_pct) FILTER (WHERE rn <= 20) AS atr_pct_20
          FROM win
         WHERE rn <= 20
         GROUP BY symbol
        HAVING COUNT(*) >= 15
        "#,
    )
    .bind(exchange)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        tracing::warn!("no bars — skipping regime snapshot");
        return Ok(());
    }

    let mut above = 0i64;
    let mut total = 0i64;
    let mut momenta = Vec::<f64>::new();
    let mut atrs = Vec::<f64>::new();
    for r in &rows {
        let c_now: Option<rust_decimal::Decimal> = r.try_get("close_now").ok();
        let c_20: Option<rust_decimal::Decimal> = r.try_get("close_20").ok();
        let atr: Option<rust_decimal::Decimal> = r.try_get("atr_pct_20").ok();
        let (Some(c_now), Some(c_20)) = (c_now, c_20) else { continue };
        if c_20.is_zero() { continue; }
        total += 1;
        if c_now > c_20 { above += 1; }
        use rust_decimal::prelude::ToPrimitive;
        let m = ((c_now / c_20 - rust_decimal::Decimal::ONE) * rust_decimal::Decimal::ONE_HUNDRED)
            .to_f64()
            .unwrap_or(0.0);
        momenta.push(m);
        if let Some(a) = atr.and_then(|a| a.to_f64()) {
            atrs.push(a);
        }
    }
    if total == 0 {
        tracing::warn!("no valid samples");
        return Ok(());
    }

    let breadth = (above as f64 / total as f64) * 100.0;
    let momentum = median(&mut momenta);
    let vol_index = median(&mut atrs);

    let regime = classify(breadth, momentum, &thresh);
    let trend = if momentum > 0.5 { "up" } else if momentum < -0.5 { "down" } else { "chop" };

    let day = Utc::now().date_naive();
    sqlx::query(
        r#"INSERT INTO qtss_market_regime_daily
               (day, exchange, sector, regime, breadth_pct, momentum_20d,
                volatility_index, dominant_trend, source)
           VALUES ($1, $2, '*', $3, $4, $5, $6, $7, 'market_regime_tick')
           ON CONFLICT (day, exchange, sector) DO UPDATE SET
               regime = EXCLUDED.regime,
               breadth_pct = EXCLUDED.breadth_pct,
               momentum_20d = EXCLUDED.momentum_20d,
               volatility_index = EXCLUDED.volatility_index,
               dominant_trend = EXCLUDED.dominant_trend,
               updated_at = now()"#,
    )
    .bind(day)
    .bind(exchange)
    .bind(regime)
    .bind(breadth)
    .bind(momentum)
    .bind(vol_index)
    .bind(trend)
    .execute(pool)
    .await?;

    tracing::info!(
        exchange, regime, breadth, momentum, vol_index, trend, samples = total,
        "regime snapshot written"
    );
    Ok(())
}

fn classify(breadth: f64, momentum: f64, t: &BTreeMap<String, f64>) -> &'static str {
    let ron_b = t.get("risk_on_breadth").copied().unwrap_or(70.0);
    let ron_m = t.get("risk_on_momentum_pct").copied().unwrap_or(3.0);
    let neu_b = t.get("neutral_breadth").copied().unwrap_or(50.0);
    let roff_b = t.get("risk_off_breadth").copied().unwrap_or(25.0);
    match (breadth, momentum) {
        (b, m) if b >= ron_b && m >= ron_m => "risk_on",
        (b, _) if b >= neu_b => "neutral",
        (b, _) if b >= roff_b => "risk_off",
        _ => "panic",
    }
}

fn median(xs: &mut [f64]) -> f64 {
    if xs.is_empty() { return 0.0; }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs[xs.len() / 2]
}
