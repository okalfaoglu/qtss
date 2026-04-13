//! Projection validation loop.
//!
//! Two responsibilities:
//! 1. **Generate**: When a new formation completes (wave_chain segments
//!    with state=active), produce projection alternatives.
//! 2. **Validate**: Every tick, check active projections against the
//!    latest candle data. Eliminate, update probability, or confirm.

use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use qtss_elliott::projection_engine::{self, ProjectionContext};
use qtss_storage::wave_projections::{
    self, ProjectedLeg, WaveProjectionInsert, WaveProjectionRow,
};

// ─── Generation ─────────────────────────────────────────────────────

/// For a completed/active formation in wave_chain, generate projection
/// alternatives and persist them. Skips if projections already exist.
pub async fn generate_projections_for_wave(
    pool: &PgPool,
    source_wave_id: Uuid,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    degree: &str,
    subkind: &str,
    prices: &[f64],
    avg_bar_spacing: u64,
    wave_number: Option<&str>,
    sibling_w2_kind: Option<&str>,
    last_time: Option<DateTime<Utc>>,
) -> anyhow::Result<usize> {
    // Skip if already projected
    let existing = wave_projections::count_active_by_source(pool, source_wave_id).await?;
    if existing > 0 {
        return Ok(0);
    }

    let ctx = ProjectionContext {
        subkind: subkind.to_string(),
        prices: prices.to_vec(),
        avg_bar_spacing,
        wave_number: wave_number.map(|s| s.to_string()),
        sibling_w2_kind: sibling_w2_kind.map(|s| s.to_string()),
    };

    let alternatives = projection_engine::project_alternatives(&ctx);
    if alternatives.is_empty() {
        return Ok(0);
    }

    let alt_group = Uuid::new_v4();
    let mut count = 0;

    for (rank, alt) in alternatives.iter().enumerate() {
        let bar_secs = estimate_bar_seconds(timeframe);
        let mut cursor = last_time; // cumulative time cursor
        let legs_json: Vec<ProjectedLeg> = alt.legs.iter().map(|leg| {
            let leg_start = cursor;
            let leg_end = cursor.map(|t| t + Duration::seconds(leg.bar_duration as i64 * bar_secs));
            cursor = leg_end; // advance cursor for next leg

            ProjectedLeg {
                label: leg.label.clone(),
                price_start: leg.price_start,
                price_end: leg.price_end,
                time_start_est: leg_start.map(|t| t.to_rfc3339()),
                time_end_est: leg_end.map(|t| t.to_rfc3339()),
                fib_level: leg.fib_level.clone(),
                direction: leg.direction.to_string(),
            }
        }).collect();

        let price_min = alt.legs.iter()
            .flat_map(|l| [l.price_start, l.price_end])
            .fold(f64::INFINITY, f64::min);
        let price_max = alt.legs.iter()
            .flat_map(|l| [l.price_start, l.price_end])
            .fold(f64::NEG_INFINITY, f64::max);

        // Estimate overall time range
        let total_bars: u64 = alt.legs.iter().map(|l| l.bar_duration).sum();
        let bar_secs = estimate_bar_seconds(timeframe);
        let time_start_est = last_time;
        let time_end_est = last_time.map(|t| t + Duration::seconds(total_bars as i64 * bar_secs));

        let insert = WaveProjectionInsert {
            source_wave_id,
            alt_group,
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            degree: degree.to_string(),
            projected_kind: alt.projected_kind.clone(),
            projected_label: alt.projected_label.clone(),
            direction: alt.direction.to_string(),
            fib_basis: Some(alt.fib_basis.clone()),
            projected_legs: serde_json::to_value(&legs_json).unwrap_or_default(),
            probability: alt.probability,
            rank: (rank + 1) as i32,
            time_start_est,
            time_end_est,
            price_target_min: Decimal::from_f64(price_min),
            price_target_max: Decimal::from_f64(price_max),
            invalidation_price: alt.invalidation_price.and_then(Decimal::from_f64),
        };

        match wave_projections::insert_projection(pool, &insert).await {
            Ok(_) => count += 1,
            Err(e) => tracing::warn!(%e, "failed to insert projection"),
        }
    }

    if count > 0 {
        tracing::info!(
            symbol, timeframe, degree, subkind,
            alternatives = count,
            "projections generated"
        );
    }

    Ok(count)
}

// ─── Validation ─────────────────────────────────────────────────────

/// Validate active projections against latest price data.
/// Called every orchestrator tick.
pub async fn validate_projections(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    current_price: f64,
    current_time: DateTime<Utc>,
) -> anyhow::Result<()> {
    let projections = wave_projections::list_active_projections(
        pool, exchange, symbol, timeframe,
    ).await?;

    if projections.is_empty() {
        return Ok(());
    }

    // Group by alt_group to handle cross-alternative probability updates
    let mut groups: std::collections::HashMap<Uuid, Vec<WaveProjectionRow>> =
        std::collections::HashMap::new();
    for p in projections {
        groups.entry(p.alt_group).or_default().push(p);
    }

    for (alt_group, alts) in &groups {
        let mut any_eliminated = false;

        for proj in alts {
            if proj.state == "eliminated" || proj.state == "confirmed" {
                continue;
            }

            let bars = proj.bars_validated + 1;

            // Check invalidation price
            if let Some(inv_price) = &proj.invalidation_price {
                let inv_f = inv_price.to_f64().unwrap_or(0.0);
                let breached = if proj.direction == "bullish" {
                    current_price < inv_f
                } else {
                    current_price > inv_f
                };

                if breached {
                    wave_projections::eliminate_projection(
                        pool, proj.id, "price_breach",
                    ).await?;
                    any_eliminated = true;
                    tracing::info!(
                        id = %proj.id,
                        kind = %proj.projected_kind,
                        "projection eliminated: price breach"
                    );
                    continue;
                }
            }

            // Check time exceeded (3× estimated duration past end_est)
            if let Some(end_est) = proj.time_end_est {
                let duration = end_est - proj.time_start_est.unwrap_or(proj.created_at);
                let max_time = end_est + duration * 2;
                if current_time > max_time {
                    wave_projections::eliminate_projection(
                        pool, proj.id, "time_exceeded",
                    ).await?;
                    any_eliminated = true;
                    tracing::info!(
                        id = %proj.id,
                        kind = %proj.projected_kind,
                        "projection eliminated: time exceeded"
                    );
                    continue;
                }
            }

            // Update probability based on how price aligns with projection
            let new_prob = adjust_probability(proj, current_price);
            wave_projections::update_validation(pool, proj.id, bars, new_prob).await?;
        }

        // Recalculate ranks if anything changed
        if any_eliminated {
            wave_projections::recalculate_ranks(pool, *alt_group).await?;
        }
    }

    Ok(())
}

/// Adjust probability based on current price vs projected path.
fn adjust_probability(proj: &WaveProjectionRow, current_price: f64) -> f32 {
    let legs: Vec<ProjectedLeg> = serde_json::from_value(proj.projected_legs.clone())
        .unwrap_or_default();

    if legs.is_empty() {
        return proj.probability;
    }

    // Check if price is moving in the projected direction
    let first_leg = &legs[0];
    let expected_dir = first_leg.price_end > first_leg.price_start;
    let actual_dir = current_price > first_leg.price_start;
    let direction_match = expected_dir == actual_dir;

    // Distance from expected path (how close to first leg target?)
    let leg_range = (first_leg.price_end - first_leg.price_start).abs();
    if leg_range == 0.0 {
        return proj.probability;
    }
    let progress = (current_price - first_leg.price_start).abs() / leg_range;
    let progress_clamped = progress.min(2.0); // cap at 2x

    let mut new_prob = proj.probability;
    if direction_match {
        // Moving in right direction — slight increase
        new_prob = (new_prob + 0.02 * progress_clamped as f32).min(0.95);
    } else {
        // Moving wrong — decrease
        new_prob = (new_prob - 0.03 * progress_clamped as f32).max(0.05);
    }

    new_prob
}

// ─── Helpers ────────────────────────────────────────────────────────

fn estimate_bar_seconds(timeframe: &str) -> i64 {
    match timeframe {
        "1m" => 60,
        "3m" => 180,
        "5m" => 300,
        "15m" => 900,
        "30m" => 1800,
        "1h" => 3600,
        "2h" => 7200,
        "4h" => 14400,
        "6h" => 21600,
        "8h" => 28800,
        "12h" => 43200,
        "1d" => 86400,
        "3d" => 259200,
        "1w" => 604800,
        "1M" => 2592000,
        _ => 3600,
    }
}
