//! Faz 8.3 — auto-refresh commission bps from Binance per (venue, side).
//!
//! Why: Faz 8.1 unified the gate around `resolve_commission_bps`, but the
//! values still default to the tier-0 seed from migration 0110. VIP /
//! BNB-discount operators have to hand-edit the row every time their tier
//! changes. This loop periodically queries the signed endpoints
//! (`/fapi/v1/commissionRate`, `/sapi/v1/asset/tradeFee`) using the first
//! Binance exchange account on record, converts the ratios to bps, and
//! upserts into `system_config` so `resolve_commission_bps` picks them up
//! without a deploy (CLAUDE.md #2).
//!
//! Config keys (module = `setup`, like the other commission rows):
//!   * `commission.sync.enabled`              — bool, default `false`
//!   * `commission.sync.interval_hours`       — u64, default `24`
//!   * `commission.sync.representative_symbol` — string, default `BTCUSDT`
//!
//! The loop is enabled-off by default because it needs real creds. When
//! no account is found for a segment we log at debug and skip — the loop
//! never blocks the worker boot.

use std::time::Duration;

use qtss_binance::{
    commission_rate_from_fapi_response, trade_fee_from_sapi_response, BinanceClient,
    BinanceClientConfig,
};
use qtss_storage::{
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
    ExchangeAccountRepository, ExchangeCredentials, SystemConfigRepository,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};

/// One (venue_class, segment) tuple the loop refreshes. Kept as a table
/// so adding a new exchange = one row, not a new match arm (CLAUDE.md #1).
struct SyncTarget {
    /// `venue_class` string written into `qtss_setups` and read by the
    /// commission resolver; e.g. `binance_futures`.
    venue_class: &'static str,
    /// `exchange_accounts.segment` value used to select creds.
    account_segment: &'static str,
    /// Which signed endpoint to hit.
    endpoint: CommissionEndpoint,
}

#[derive(Clone, Copy)]
enum CommissionEndpoint {
    FapiCommissionRate,
    SapiAssetTradeFee,
}

const TARGETS: &[SyncTarget] = &[
    SyncTarget {
        venue_class: "binance_futures",
        account_segment: "futures",
        endpoint: CommissionEndpoint::FapiCommissionRate,
    },
    SyncTarget {
        venue_class: "binance_spot",
        account_segment: "spot",
        endpoint: CommissionEndpoint::SapiAssetTradeFee,
    },
];

pub async fn commission_sync_loop(pool: PgPool) {
    info!("commission_sync_loop: Binance commission auto-refresh (Faz 8.3)");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "setup",
            "commission.sync.enabled",
            "QTSS_COMMISSION_SYNC_ENABLED",
            false,
        )
        .await;
        let interval_hours = resolve_system_u64(
            &pool,
            "setup",
            "commission.sync.interval_hours",
            "QTSS_COMMISSION_SYNC_INTERVAL_HOURS",
            24,
            1,
            24 * 30,
        )
        .await;
        let symbol = resolve_system_string(
            &pool,
            "setup",
            "commission.sync.representative_symbol",
            "QTSS_COMMISSION_SYNC_SYMBOL",
            "BTCUSDT",
        )
        .await;

        if enabled {
            run_pass(&pool, symbol.trim()).await;
        }

        // Always sleep at least 15 minutes even if disabled, so the flag
        // is re-read often enough to pick up GUI toggles.
        let sleep_secs = if enabled {
            interval_hours.saturating_mul(3600).max(900)
        } else {
            900
        };
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}

async fn run_pass(pool: &PgPool, symbol: &str) {
    let accts = ExchangeAccountRepository::new(pool.clone());
    let cfg_repo = SystemConfigRepository::new(pool.clone());
    for t in TARGETS {
        match refresh_target(pool, &accts, &cfg_repo, t, symbol).await {
            Ok(Some((maker_bps, taker_bps))) => info!(
                venue = t.venue_class,
                maker_bps,
                taker_bps,
                "commission_sync: upserted"
            ),
            Ok(None) => debug!(
                venue = t.venue_class,
                "commission_sync: no account / no rate — skipped"
            ),
            Err(e) => warn!(%e, venue = t.venue_class, "commission_sync: failed"),
        }
    }
}

async fn refresh_target(
    pool: &PgPool,
    accts: &ExchangeAccountRepository,
    cfg_repo: &SystemConfigRepository,
    target: &SyncTarget,
    symbol: &str,
) -> anyhow::Result<Option<(f64, f64)>> {
    let Some(creds) = first_binance_creds(accts, target.account_segment).await? else {
        return Ok(None);
    };
    let client = BinanceClient::new(BinanceClientConfig::mainnet_with_keys(
        creds.api_key,
        creds.api_secret,
    ))
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let Some((maker_dec, taker_dec)) = fetch_rates(&client, target.endpoint, symbol).await? else {
        return Ok(None);
    };
    let maker_bps = ratio_to_bps(maker_dec);
    let taker_bps = ratio_to_bps(taker_dec);
    if !maker_bps.is_finite() || !taker_bps.is_finite() || maker_bps < 0.0 || taker_bps < 0.0 {
        return Ok(None);
    }
    upsert_bps(cfg_repo, target.venue_class, "maker_bps", maker_bps).await?;
    upsert_bps(cfg_repo, target.venue_class, "taker_bps", taker_bps).await?;
    let _ = pool; // reserved for audit hook
    Ok(Some((maker_bps, taker_bps)))
}

async fn first_binance_creds(
    accts: &ExchangeAccountRepository,
    segment: &str,
) -> anyhow::Result<Option<ExchangeCredentials>> {
    let users = accts
        .list_user_ids_binance_segment(segment)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    for uid in users {
        let c = accts
            .binance_for_user(uid, segment)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(c) = c {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

async fn fetch_rates(
    client: &BinanceClient,
    endpoint: CommissionEndpoint,
    symbol: &str,
) -> anyhow::Result<Option<(Decimal, Decimal)>> {
    match endpoint {
        CommissionEndpoint::FapiCommissionRate => {
            let v = client
                .fapi_commission_rate(symbol)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Ok(commission_rate_from_fapi_response(&v))
        }
        CommissionEndpoint::SapiAssetTradeFee => {
            let v = client
                .sapi_asset_trade_fee(Some(symbol))
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Ok(trade_fee_from_sapi_response(&v, symbol))
        }
    }
}

/// Binance returns a fractional rate (`0.0004` = 4 bps). Convert and clamp
/// absurd values so a stale/garbled response can't poison the gate.
fn ratio_to_bps(rate: Decimal) -> f64 {
    let f = rate.to_f64().unwrap_or(f64::NAN);
    let bps = f * 10_000.0;
    if bps.is_finite() && (0.0..=200.0).contains(&bps) {
        bps
    } else {
        f64::NAN
    }
}

async fn upsert_bps(
    cfg_repo: &SystemConfigRepository,
    venue_class: &str,
    side_key: &str,
    bps: f64,
) -> anyhow::Result<()> {
    let key = format!("commission.{venue_class}.{side_key}");
    let desc = format!("Auto-synced from Binance API ({venue_class} {side_key}).");
    cfg_repo
        .upsert(
            "setup",
            &key,
            json!(bps),
            None,
            Some(&desc),
            Some(false),
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_to_bps_converts_fraction() {
        let r = Decimal::new(4, 4); // 0.0004
        assert!((ratio_to_bps(r) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn ratio_to_bps_rejects_absurd() {
        let r = Decimal::new(5, 1); // 0.5 → 5000 bps, out of range
        assert!(ratio_to_bps(r).is_nan());
    }

    #[test]
    fn ratio_to_bps_rejects_negative() {
        let r = Decimal::new(-1, 4);
        assert!(ratio_to_bps(r).is_nan());
    }
}
