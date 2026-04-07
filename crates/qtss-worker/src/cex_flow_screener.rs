//! CEX flow screeners derived from latest `nansen_netflows` snapshot (Nansen smart-money netflow).
//!
//! Two independent jobs (enable separately):
//! - [`crate::data_sources::registry::CEX_FLOW_ACCUMULATION_REPORT_KEY`] — CEX OUTFLOW / accumulation bias.
//! - [`crate::data_sources::registry::CEX_FLOW_DISTRIBUTION_REPORT_KEY`] — CEX INFLOW / distribution (dump) bias.
//!
//! Writes JSON to `data_snapshots`. Optional Telegram via `notify_outbox` with separate notify toggles.

use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_notify::{escape_telegram_html, NotificationChannel};
use qtss_storage::{
    fetch_data_snapshot, resolve_system_csv, resolve_system_u64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, upsert_data_snapshot, NotifyOutboxRepository,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::{
    CEX_FLOW_ACCUMULATION_REPORT_KEY, CEX_FLOW_DISTRIBUTION_REPORT_KEY, NANSEN_NETFLOWS_DATA_KEY,
};

const WORKER_MODULE: &str = "worker";

const TITLE_ACCUMULATION: &str = "CEX OUTFLOW · Accumulation (TOP 25 · 24H)";
const TITLE_DISTRIBUTION: &str = "CEX INFLOW · Distribution / dump risk (TOP 25 · 24H)";

fn pick_f64(row: &Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(v) = row.get(*k) {
            if let Some(x) = v.as_f64() {
                return Some(x);
            }
            if let Some(s) = v.as_str() {
                if let Ok(x) = s.trim().parse::<f64>() {
                    return Some(x);
                }
            }
            if let Some(i) = v.as_i64() {
                return Some(i as f64);
            }
        }
    }
    None
}

fn pick_u64(row: &Value, keys: &[&str]) -> Option<u64> {
    for k in keys {
        if let Some(v) = row.get(*k) {
            if let Some(u) = v.as_u64() {
                return Some(u);
            }
            if let Some(s) = v.as_str() {
                if let Ok(u) = s.trim().parse::<u64>() {
                    return Some(u);
                }
            }
            if let Some(i) = v.as_i64() {
                if i >= 0 {
                    return Some(i as u64);
                }
            }
        }
    }
    None
}

fn row_symbol(row: &Value) -> Option<String> {
    let s = row
        .get("symbol")
        .or_else(|| row.get("token_symbol"))
        .or_else(|| row.get("tokenSymbol"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(s.to_uppercase())
}

/// When gross flows exist, use them. Otherwise derive from signed `net_flow` with `net ≈ outflow - inflow`.
fn row_flow_triple(row: &Value) -> (f64, f64, f64) {
    let inflow = pick_f64(
        row,
        &[
            "inflow_usd",
            "inflowUsd",
            "inflow",
            "cex_inflow_usd",
            "exchange_inflow_usd",
            "to_exchange_usd",
            "toExchangeUsd",
        ],
    );
    let outflow = pick_f64(
        row,
        &[
            "outflow_usd",
            "outflowUsd",
            "outflow",
            "cex_outflow_usd",
            "exchange_outflow_usd",
            "from_exchange_usd",
            "fromExchangeUsd",
        ],
    );
    let net = pick_f64(
        row,
        &[
            "net_flow",
            "netFlow",
            "net_volume",
            "netUsd",
            "net_usd",
            "netflow_usd",
        ],
    );

    if let (Some(i), Some(o)) = (inflow, outflow) {
        let n = net.unwrap_or(o - i);
        return (i.max(0.0), o.max(0.0), n);
    }

    if let Some(n) = net {
        let o = n.max(0.0);
        let i = (-n).max(0.0);
        return (i, o, n);
    }

    (0.0, 0.0, 0.0)
}

fn row_volume_usd(row: &Value) -> Option<f64> {
    pick_f64(
        row,
        &[
            "volume_24h_usd",
            "volumeUsd24h",
            "volume_usd_24h",
            "volume_usd",
            "vol_24h_usd",
            "volume24h",
        ],
    )
    .filter(|v| v.is_finite() && *v > 0.0)
}

fn row_mcap_usd(row: &Value) -> Option<f64> {
    pick_f64(
        row,
        &["market_cap_usd", "marketCapUsd", "market_cap", "marketCap", "mcap_usd"],
    )
    .filter(|v| v.is_finite() && *v > 0.0)
}

fn whale_withdrawals(row: &Value) -> Option<u64> {
    pick_u64(
        row,
        &[
            "whale_withdrawal_count",
            "whale_withdrawals_24h",
            "large_withdrawal_count",
            "whale_outflow_tx_count",
        ],
    )
}

fn whale_deposits(row: &Value) -> Option<u64> {
    pick_u64(
        row,
        &[
            "whale_deposit_count",
            "whale_deposits_24h",
            "large_deposit_count",
            "whale_inflow_tx_count",
        ],
    )
}

fn smart_money_net(row: &Value) -> Option<f64> {
    pick_f64(
        row,
        &[
            "smart_money_net_flow",
            "smartMoneyNetFlow",
            "sm_net_flow",
            "smart_net_flow",
        ],
    )
}

fn is_blocked_symbol(sym: &str) -> bool {
    let t = sym.trim().to_uppercase();
    matches!(
        t.as_str(),
        "USDT"
            | "USDC"
            | "DAI"
            | "BUSD"
            | "TUSD"
            | "USDD"
            | "FDUSD"
            | "GUSD"
            | "PYUSD"
            | "USDP"
            | "LUSD"
            | "WETH"
            | "WBTC"
            | "STETH"
            | "WSTETH"
            | "WEETH"
            | "WBNB"
            | "WMATIC"
            | "WAVAX"
    )
}

fn passes_mcap_band(mcap: Option<f64>) -> bool {
    let Some(m) = mcap else {
        return true;
    };
    m >= 5_000_000.0 && m <= 1_000_000_000.0
}

fn netflow_tokens_rows(v: &Value) -> Vec<&Value> {
    let data = match v.get("data") {
        Some(d) => d,
        None => return vec![],
    };
    if let Some(a) = data.as_array() {
        return a.iter().collect();
    }
    vec![data]
}

#[derive(Debug, Clone)]
struct ParsedRow {
    symbol: String,
    inflow_usd: f64,
    outflow_usd: f64,
    net_flow_usd: f64,
    volume_24h_usd: Option<f64>,
    market_cap_usd: Option<f64>,
    whale_withdrawals: Option<u64>,
    whale_deposits: Option<u64>,
    sm_net: Option<f64>,
}

fn parse_rows(netflows: &Value) -> (Vec<ParsedRow>, Vec<String>) {
    let mut notes = Vec::new();
    let mut rows = Vec::new();
    for row in netflow_tokens_rows(netflows) {
        let Some(sym) = row_symbol(row) else {
            continue;
        };
        if is_blocked_symbol(&sym) {
            continue;
        }
        let (i, o, n) = row_flow_triple(row);
        if i.abs() < f64::EPSILON && o.abs() < f64::EPSILON && n.abs() < f64::EPSILON {
            continue;
        }
        let mcap = row_mcap_usd(row);
        if !passes_mcap_band(mcap) {
            continue;
        }
        rows.push(ParsedRow {
            symbol: sym,
            inflow_usd: i,
            outflow_usd: o,
            net_flow_usd: n,
            volume_24h_usd: row_volume_usd(row),
            market_cap_usd: mcap,
            whale_withdrawals: whale_withdrawals(row),
            whale_deposits: whale_deposits(row),
            sm_net: smart_money_net(row),
        });
    }
    if rows.is_empty() {
        notes.push(
            "no rows after filters — check nansen_netflows freshness and JSON shape (inflow/outflow/net_flow)."
                .into(),
        );
    }
    (rows, notes)
}

fn median_f64(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    if v.len() % 2 == 0 {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    }
}

fn flow_volume_ratio(primary_flow: f64, vol: Option<f64>) -> Option<f64> {
    let v = vol?;
    if v < 1.0 {
        return None;
    }
    Some((primary_flow.abs() / v * 100.0).min(10_000.0))
}

fn sm_label_accumulation(sm: Option<f64>, net: f64) -> &'static str {
    if let Some(x) = sm {
        if x > 1e-6 {
            return "buy";
        }
        if x < -1e-6 {
            return "sell";
        }
    }
    if net > 1e-6 {
        return "buy";
    }
    if net < -1e-6 {
        return "sell";
    }
    "neutral"
}

fn sm_label_distribution(sm: Option<f64>, net: f64) -> &'static str {
    if let Some(x) = sm {
        if x < -1e-6 {
            return "sell";
        }
        if x > 1e-6 {
            return "buy";
        }
    }
    if net < -1e-6 {
        return "sell";
    }
    if net > 1e-6 {
        return "buy";
    }
    "neutral"
}

fn build_ranked_payload(
    report_key: &str,
    display_name: &str,
    source_key_upstream: &str,
    source_computed_at: Option<DateTime<Utc>>,
    parsed: Vec<ParsedRow>,
    rank_by: impl Fn(&ParsedRow) -> f64,
    top_n: usize,
    notes: Vec<String>,
    sm_label_fn: fn(Option<f64>, f64) -> &'static str,
    whale_pick: fn(&ParsedRow) -> Option<u64>,
    flow_for_ratio: impl Copy + Fn(&ParsedRow) -> f64,
) -> Value {
    let mut parsed = parsed;
    parsed.sort_by(|a, b| {
        rank_by(b)
            .partial_cmp(&rank_by(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    parsed.truncate(top_n);

    let ranks: Vec<f64> = parsed.iter().map(&rank_by).filter(|x| x.is_finite()).collect();
    let med = median_f64(ranks.clone());
    let p90 = if ranks.len() >= 10 {
        let mut s = ranks.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let i = ((s.len() as f64 * 0.9) as usize).min(s.len().saturating_sub(1));
        s[i]
    } else {
        med
    };

    let out_rows: Vec<Value> = parsed
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let rank = idx + 1;
            let pr = rank_by(r);
            let extreme = pr > 0.0 && (pr > med * 3.0 || pr > p90 * 1.5);
            let base_score =
                (((top_n.saturating_sub(rank).saturating_add(1)) as f64 / top_n as f64) * 9.0).round()
                    as i32
                    + 1;
            let score_1_10 = (base_score + if extreme { 1 } else { 0 }).clamp(1, 10);
            let sm = sm_label_fn(r.sm_net, r.net_flow_usd);
            let wh = whale_pick(r);
            let fv = flow_for_ratio(r);
            let vr = flow_volume_ratio(fv, r.volume_24h_usd);

            json!({
                "rank": rank,
                "symbol": r.symbol,
                "primary_flow_usd": pr,
                "total_inflow_usd": r.inflow_usd,
                "total_outflow_usd": r.outflow_usd,
                "net_flow_usd": r.net_flow_usd,
                "whale_count": wh,
                "smart_money_activity": sm,
                "flow_to_volume_pct": vr,
                "market_cap_usd": r.market_cap_usd,
                "extreme_spike": extreme,
                "score_1_10": score_1_10,
            })
        })
        .collect();

    json!({
        "report_name": report_key,
        "display_name": display_name,
        "window_label": "24h",
        "source_snapshot_key": source_key_upstream,
        "source_computed_at": source_computed_at,
        "top_n": top_n,
        "top": out_rows,
        "parsing_notes": notes,
        "conventions": {
            "net_flow_sign": "When only net_flow is present: outflow=max(0,net), inflow=max(0,-net); pairing is approximate.",
            "filters": "Stable / wrapped symbols excluded; mcap 5M–1B when market_cap_usd is present on row.",
        },
    })
}

async fn recent_global_event(pool: &PgPool, event_key: &str, lookback_secs: i64) -> bool {
    if lookback_secs <= 0 {
        return false;
    }
    match sqlx::query_as::<_, (bool,)>(
        r#"SELECT EXISTS (
            SELECT 1 FROM notify_outbox
            WHERE org_id IS NULL
              AND event_key = $1
              AND created_at > now() - ($2 * interval '1 second')
        )"#,
    )
    .bind(event_key)
    .bind(lookback_secs)
    .fetch_one(pool)
    .await
    {
        Ok((x,)) => x,
        Err(e) => {
            warn!(%e, %event_key, "cex_flow_screener: recent_global_event");
            true
        }
    }
}

fn channel_list_csv(channels: &[NotificationChannel]) -> Vec<String> {
    channels.iter().map(|c| c.as_str().to_string()).collect()
}

fn format_telegram_html(title: &str, payload: &Value) -> String {
    let mut lines = vec![format!("<b>{}</b>", escape_telegram_html(title)), String::new()];
    if let Some(arr) = payload.get("top").and_then(|x| x.as_array()) {
        for row in arr.iter().take(25) {
            let sym = row
                .get("symbol")
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let rank = row.get("rank").and_then(|x| x.as_u64()).unwrap_or(0);
            let net = row
                .get("net_flow_usd")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let spike = row
                .get("extreme_spike")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let sc = row
                .get("score_1_10")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let wh = row
                .get("whale_count")
                .and_then(|x| x.as_u64())
                .map(|u| u.to_string())
                .unwrap_or_else(|| "—".into());
            let sm = row
                .get("smart_money_activity")
                .and_then(|x| x.as_str())
                .unwrap_or("—");
            let fv = row
                .get("flow_to_volume_pct")
                .and_then(|x| x.as_f64())
                .map(|p| format!("{p:.2}%"))
                .unwrap_or_else(|| "—".into());
            let spike_s = if spike { " ⚠️" } else { "" };
            lines.push(format!(
                "{}. <code>{}</code> net={:.0} wh={} sm={} vol%={} sc={}{}",
                rank,
                escape_telegram_html(sym),
                net,
                wh,
                escape_telegram_html(sm),
                escape_telegram_html(&fv),
                sc,
                spike_s
            ));
        }
    }
    lines.join("\n")
}

async fn maybe_enqueue(
    pool: &PgPool,
    notify_on: bool,
    event_key: &str,
    title: &str,
    payload: &Value,
    channels: &[NotificationChannel],
    lookback_secs: i64,
) {
    if !notify_on || channels.is_empty() {
        return;
    }
    if recent_global_event(pool, event_key, lookback_secs).await {
        return;
    }
    let body = format_telegram_html(title, payload);
    let repo = NotifyOutboxRepository::new(pool.clone());
    let ch = channel_list_csv(channels);
    if let Err(e) = repo
        .enqueue_with_meta(
            None,
            Some(event_key),
            "info",
            None,
            None,
            None,
            title,
            &body,
            ch,
        )
        .await
    {
        warn!(%e, %event_key, "cex_flow_screener: enqueue");
    }
}

pub async fn cex_flow_screener_loop(pool: PgPool) {
    info!("cex_flow_screener_loop: optional accumulation + distribution reports → data_snapshots");

    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            WORKER_MODULE,
            "cex_flow_screener_tick_secs",
            "QTSS_CEX_FLOW_SCREENER_TICK_SECS",
            3600,
            300,
        )
        .await;
        let top_n = resolve_system_u64(
            &pool,
            WORKER_MODULE,
            "cex_flow_screener_top_n",
            "QTSS_CEX_FLOW_SCREENER_TOP_N",
            25,
            5,
            100,
        )
        .await as usize;

        let acc_on = resolve_worker_enabled_flag(
            &pool,
            WORKER_MODULE,
            "cex_flow_accumulation_screener_enabled",
            "QTSS_CEX_FLOW_ACCUMULATION_SCREENER_ENABLED",
            false,
        )
        .await;
        let dist_on = resolve_worker_enabled_flag(
            &pool,
            WORKER_MODULE,
            "cex_flow_distribution_screener_enabled",
            "QTSS_CEX_FLOW_DISTRIBUTION_SCREENER_ENABLED",
            false,
        )
        .await;

        if !acc_on && !dist_on {
            continue;
        }

        let snap = match fetch_data_snapshot(&pool, NANSEN_NETFLOWS_DATA_KEY).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "cex_flow_screener: fetch nansen_netflows");
                continue;
            }
        };
        let Some(snap) = snap else {
            warn!("cex_flow_screener: no nansen_netflows snapshot");
            continue;
        };
        let Some(resp) = snap.response_json.as_ref() else {
            warn!("cex_flow_screener: nansen_netflows response_json empty");
            continue;
        };

        let (parsed, notes) = parse_rows(resp);
        let source_at = Some(snap.computed_at);

        let notify_channels: Vec<NotificationChannel> = resolve_system_csv(
            &pool,
            WORKER_MODULE,
            "cex_flow_screener_notify_channels_csv",
            "QTSS_CEX_FLOW_SCREENER_NOTIFY_CHANNELS",
            "telegram",
        )
        .await
        .iter()
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect();

        let acc_notify = resolve_worker_enabled_flag(
            &pool,
            WORKER_MODULE,
            "cex_flow_accumulation_notify_enabled",
            "QTSS_CEX_FLOW_ACCUMULATION_NOTIFY_ENABLED",
            false,
        )
        .await;
        let dist_notify = resolve_worker_enabled_flag(
            &pool,
            WORKER_MODULE,
            "cex_flow_distribution_notify_enabled",
            "QTSS_CEX_FLOW_DISTRIBUTION_NOTIFY_ENABLED",
            false,
        )
        .await;

        let lookback = (tick as i64).saturating_sub(30).max(60);

        if acc_on {
            let payload = build_ranked_payload(
                CEX_FLOW_ACCUMULATION_REPORT_KEY,
                TITLE_ACCUMULATION,
                NANSEN_NETFLOWS_DATA_KEY,
                source_at,
                parsed.clone(),
                |r| r.outflow_usd,
                top_n,
                notes.clone(),
                sm_label_accumulation,
                |r| r.whale_withdrawals,
                |r| r.outflow_usd,
            );
            if let Err(e) = upsert_data_snapshot(
                &pool,
                CEX_FLOW_ACCUMULATION_REPORT_KEY,
                &json!({ "upstream": NANSEN_NETFLOWS_DATA_KEY }),
                Some(&payload),
                None,
                None,
            )
            .await
            {
                warn!(%e, "cex_flow_screener: upsert accumulation");
            } else {
                info!(
                    key = CEX_FLOW_ACCUMULATION_REPORT_KEY,
                    n = payload.get("top").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0),
                    "cex_flow_screener: accumulation report"
                );
            }
            maybe_enqueue(
                &pool,
                acc_notify,
                "cex_flow_accumulation_top25",
                TITLE_ACCUMULATION,
                &payload,
                &notify_channels,
                lookback,
            )
            .await;
        }

        if dist_on {
            let payload = build_ranked_payload(
                CEX_FLOW_DISTRIBUTION_REPORT_KEY,
                TITLE_DISTRIBUTION,
                NANSEN_NETFLOWS_DATA_KEY,
                source_at,
                parsed,
                |r| r.inflow_usd,
                top_n,
                notes,
                sm_label_distribution,
                |r| r.whale_deposits,
                |r| r.inflow_usd,
            );
            if let Err(e) = upsert_data_snapshot(
                &pool,
                CEX_FLOW_DISTRIBUTION_REPORT_KEY,
                &json!({ "upstream": NANSEN_NETFLOWS_DATA_KEY }),
                Some(&payload),
                None,
                None,
            )
            .await
            {
                warn!(%e, "cex_flow_screener: upsert distribution");
            } else {
                info!(
                    key = CEX_FLOW_DISTRIBUTION_REPORT_KEY,
                    n = payload.get("top").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0),
                    "cex_flow_screener: distribution report"
                );
            }
            maybe_enqueue(
                &pool,
                dist_notify,
                "cex_flow_distribution_top25",
                TITLE_DISTRIBUTION,
                &payload,
                &notify_channels,
                lookback,
            )
            .await;
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_net_only_rows_rank_outflow() {
        let v = json!({
            "data": [
                { "symbol": "AAA", "net_flow": 1000.0, "market_cap_usd": 10_000_000.0 },
                { "symbol": "BBB", "net_flow": 500.0, "market_cap_usd": 20_000_000.0 },
                { "symbol": "USDT", "net_flow": 9e9 },
            ]
        });
        let (rows, _) = parse_rows(&v);
        assert_eq!(rows.len(), 2);
        let payload = build_ranked_payload(
            "cex_flow_accumulation_top25",
            "t",
            "nansen_netflows",
            None,
            rows,
            |r| r.outflow_usd,
            25,
            vec![],
            sm_label_accumulation,
            |_| None,
            |r| r.outflow_usd,
        );
        let top = payload["top"].as_array().unwrap();
        assert_eq!(top[0]["symbol"], "AAA");
        assert_eq!(top[0]["primary_flow_usd"], 1000.0);
    }
}
