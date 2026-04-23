//! Pivot writer — ports `pivot_writer_loop` into the engine dispatch.
//!
//! Per tick: for every enabled symbol, pull the most recent ~2k bars,
//! run `qtss_pivots::zigzag::compute_pivots` across the five
//! slot lengths, and upsert each *confirmed* pivot (not the running
//! head) into the canonical `pivots` table with an HH/HL/LL/LH swing
//! tag relative to the previous same-direction pivot.

use async_trait::async_trait;
use qtss_pivots::zigzag::{compute_pivots, Sample};
use qtss_storage::market_bars;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{debug, warn};

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct PivotWriter;

#[async_trait]
impl WriterTask for PivotWriter {
    fn family_name(&self) -> &'static str {
        "pivot"
    }

    async fn is_enabled(&self, pool: &PgPool) -> bool {
        // Legacy key name (`worker.pivot_writer_enabled`) — kept so an
        // operator who previously disabled the old loop via that flag
        // still disables this writer without migrating config.
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT value FROM system_config
               WHERE module = 'worker' AND config_key = 'pivot_writer_enabled'",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let Some((val,)) = row else { return true; };
        val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let symbols = symbols::list_enabled(pool).await?;
        let slot_lengths = symbols::load_slot_lengths(pool).await;
        for sym in symbols {
            match process_symbol(pool, &sym, &slot_lengths).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                    debug!(
                        sym = %format!("{}/{}/{}", sym.exchange, sym.symbol, sym.interval),
                        rows = n,
                        "pivot: upserted"
                    );
                }
                Err(e) => warn!(
                    sym = %format!("{}/{}/{}", sym.exchange, sym.symbol, sym.interval),
                    %e,
                    "pivot: symbol failed"
                ),
            }
        }
        Ok(stats)
    }
}

fn swing_tag_for(
    direction: i8,
    price: Decimal,
    prev: Option<&(i8, Decimal)>,
) -> Option<&'static str> {
    let prev = prev?;
    if prev.0 != direction {
        return None;
    }
    if direction == 1 {
        Some(if price >= prev.1 { "HH" } else { "LH" })
    } else {
        Some(if price <= prev.1 { "LL" } else { "HL" })
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot_lengths: &[u32; 5],
) -> anyhow::Result<usize> {
    let bars = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        2000,
    )
    .await?;
    if bars.len() < 10 {
        return Ok(0);
    }
    // DB gave us newest-first; reverse to chronological for the port.
    let mut chrono_bars: Vec<_> = bars.into_iter().rev().collect();
    let samples: Vec<Sample> = chrono_bars
        .iter_mut()
        .enumerate()
        .map(|(i, r)| Sample {
            bar_index: i as u64,
            time: r.open_time,
            high: r.high,
            low: r.low,
            volume: r.volume,
        })
        .collect();

    let mut written = 0usize;
    for (slot_idx, length) in slot_lengths.iter().enumerate() {
        let all = compute_pivots(&samples, *length);
        // Drop the running head — it may still drift to a later bar on
        // future bars (Pine's `not dirchanged` replace path), which
        // would leave stale rows in `pivots` keyed by the old open_time.
        // Only pivots locked in by a subsequent opposite-direction pivot
        // are safe to persist.
        let confirmed: &[_] = if all.is_empty() {
            &all
        } else {
            &all[..all.len() - 1]
        };
        let mut prev_same: Option<(i8, Decimal)> = None;
        for cp in confirmed {
            let direction: i8 = cp.direction;
            let swing = swing_tag_for(direction.signum(), cp.price, prev_same.as_ref());
            prev_same = Some((direction.signum(), cp.price));
            sqlx::query(
                r#"INSERT INTO pivots
                      (engine_symbol_id, level, bar_index, open_time,
                       direction, price, volume, swing_tag, prominence)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                   ON CONFLICT (engine_symbol_id, level, open_time) DO UPDATE
                      SET bar_index   = EXCLUDED.bar_index,
                          direction   = EXCLUDED.direction,
                          price       = EXCLUDED.price,
                          volume      = EXCLUDED.volume,
                          swing_tag   = EXCLUDED.swing_tag,
                          prominence  = EXCLUDED.prominence,
                          computed_at = now()"#,
            )
            .bind(sym.id)
            .bind(slot_idx as i16)
            .bind(cp.bar_index as i64)
            .bind(cp.time)
            .bind(direction as i16)
            .bind(cp.price)
            .bind(cp.volume_at_pivot)
            .bind(swing)
            .bind(cp.prominence)
            .execute(pool)
            .await?;
            written += 1;
        }
    }
    Ok(written)
}
