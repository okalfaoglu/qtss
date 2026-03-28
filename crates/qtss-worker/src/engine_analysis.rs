//! DB’deki `engine_symbols` satırları için mum çekip analiz snapshot yazar:
//! `trading_range` + `signal_dashboard`.
//!
//! İsteğe bağlı: `QTSS_NOTIFY_ON_SWEEP=1` ve `QTSS_NOTIFY_ON_SWEEP_CHANNELS` ile yükselen sweep kenarında bildirim.

use std::time::Duration;

use chrono::Utc;
use qtss_chart_patterns::{
    analyze_trading_range, compute_signal_dashboard_v1_with_policy, OhlcBar, SignalDirectionPolicy,
    TradingRangeParams, TradingRangeResult,
};
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    fetch_analysis_snapshot_payload, insert_range_signal_event, list_enabled_engine_symbols, list_recent_bars,
    upsert_analysis_snapshot, EngineSymbolRow, RangeSignalEventInsert,
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn d_to_f(d: rust_decimal::Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

fn trading_range_params_from_env() -> TradingRangeParams {
    let lookback = std::env::var("QTSS_TR_LOOKBACK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let atr_period = std::env::var("QTSS_TR_ATR_PERIOD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14);
    let atr_sma_period = std::env::var("QTSS_TR_ATR_SMA_PERIOD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let require_range_regime = std::env::var("QTSS_TR_REQUIRE_RANGE_REGIME")
        .ok()
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes"));
    TradingRangeParams {
        lookback,
        atr_period,
        atr_sma_period,
        require_range_regime,
    }
}

fn enrich_tr_payload(
    tr: &TradingRangeResult,
    window_start_idx: usize,
    window_end_idx: usize,
    chrono_open_times: &[chrono::DateTime<chrono::Utc>],
) -> serde_json::Value {
    let mut v = serde_json::to_value(tr).unwrap_or(json!({}));
    if let serde_json::Value::Object(ref mut m) = v {
        if window_start_idx < chrono_open_times.len() {
            m.insert(
                "chart_window_start_open_time".into(),
                json!(chrono_open_times[window_start_idx].to_rfc3339()),
            );
        }
        if window_end_idx < chrono_open_times.len() {
            m.insert(
                "chart_window_end_open_time".into(),
                json!(chrono_open_times[window_end_idx].to_rfc3339()),
            );
        }
        if let Some(last) = chrono_open_times.last() {
            m.insert("last_bar_open_time".into(), json!(last.to_rfc3339()));
        }
    }
    v
}

fn signal_direction_policy_for_row(t: &EngineSymbolRow) -> SignalDirectionPolicy {
    match t.signal_direction_mode.trim().to_lowercase().as_str() {
        "both" | "bidirectional" | "long_short" | "long_and_short" => SignalDirectionPolicy::Both,
        "long_only" | "longonly" => SignalDirectionPolicy::LongOnly,
        "short_only" | "shortonly" => SignalDirectionPolicy::ShortOnly,
        "auto_segment" | "auto" | "" => {
            let seg = t.segment.trim().to_lowercase();
            if matches!(
                seg.as_str(),
                "futures" | "usdt_futures" | "fapi" | "future"
            ) {
                SignalDirectionPolicy::Both
            } else {
                SignalDirectionPolicy::LongOnly
            }
        }
        _ => {
            warn!(
                mode = %t.signal_direction_mode,
                "tanınmayan signal_direction_mode — segment ile auto_segment uygulanıyor"
            );
            let seg = t.segment.trim().to_lowercase();
            if matches!(
                seg.as_str(),
                "futures" | "usdt_futures" | "fapi" | "future"
            ) {
                SignalDirectionPolicy::Both
            } else {
                SignalDirectionPolicy::LongOnly
            }
        }
    }
}

fn enrich_dashboard_payload(
    dash: &qtss_chart_patterns::SignalDashboardV1,
    tr: &TradingRangeResult,
    last_open_time: chrono::DateTime<chrono::Utc>,
    t: &EngineSymbolRow,
    effective: SignalDirectionPolicy,
) -> serde_json::Value {
    let mut v = serde_json::to_value(dash).unwrap_or(json!({}));
    if let serde_json::Value::Object(ref mut m) = v {
        m.insert("last_bar_open_time".into(), json!(last_open_time.to_rfc3339()));
        if let Some(x) = tr.range_high {
            m.insert("range_high".into(), json!(x));
        }
        if let Some(x) = tr.range_low {
            m.insert("range_low".into(), json!(x));
        }
        if let Some(x) = tr.mid {
            m.insert("range_mid".into(), json!(x));
        }
        if let Some(x) = tr.atr {
            m.insert("atr".into(), json!(x));
        }
        m.insert(
            "signal_direction_mode".into(),
            json!(t.signal_direction_mode.as_str()),
        );
        m.insert(
            "signal_direction_effective".into(),
            json!(effective.as_api_str()),
        );
    }
    v
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes"))
}

fn sweep_notify_channels_from_env() -> Vec<NotificationChannel> {
    let raw = std::env::var("QTSS_NOTIFY_ON_SWEEP_CHANNELS").unwrap_or_else(|_| "webhook".into());
    raw.split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn sweep_notify_bundle() -> Option<(NotificationDispatcher, Vec<NotificationChannel>)> {
    if !env_truthy("QTSS_NOTIFY_ON_SWEEP") {
        return None;
    }
    let chans = sweep_notify_channels_from_env();
    if chans.is_empty() {
        warn!("QTSS_NOTIFY_ON_SWEEP açık fakat QTSS_NOTIFY_ON_SWEEP_CHANNELS boş veya tanınmadı");
        return None;
    }
    Some((NotificationDispatcher::from_env(), chans))
}

fn sweep_flags_from_payload(v: &serde_json::Value) -> (bool, bool) {
    if v.get("reason").and_then(|x| x.as_str()) == Some("insufficient_bars") {
        return (false, false);
    }
    let lg = v
        .get("long_sweep_signal")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let sh = v
        .get("short_sweep_signal")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    (lg, sh)
}

async fn notify_sweep_edge(
    d: &NotificationDispatcher,
    channels: &[NotificationChannel],
    t: &EngineSymbolRow,
    side: &str,
    tr: &TradingRangeResult,
) {
    let title = format!("{side} sweep — {} {}", t.symbol, t.interval);
    let body = format!(
        "{} / {}\nrange_high={:?}\nrange_low={:?}",
        t.exchange, t.segment, tr.range_high, tr.range_low
    );
    let n = Notification::new(title, body);
    for r in d.send_all(channels, &n).await {
        if r.ok {
            info!(channel = ?r.channel, symbol = %t.symbol, "sweep bildirimi");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, "sweep bildirimi başarısız");
        }
    }
}

/// `signal_dashboard.payload.durum` (LONG / SHORT / NOTR) kenarı → F1 olay satırları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashDurum {
    Long,
    Short,
    Notr,
}

impl DashDurum {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
            Self::Notr => "NOTR",
        }
    }
}

fn durum_from_signal_payload(v: &serde_json::Value) -> Option<DashDurum> {
    if v.get("reason").and_then(|x| x.as_str()) == Some("insufficient_bars") {
        return None;
    }
    match v.get("durum").and_then(|x| x.as_str()) {
        Some("LONG") => Some(DashDurum::Long),
        Some("SHORT") => Some(DashDurum::Short),
        Some("NOTR") => Some(DashDurum::Notr),
        _ => None,
    }
}

fn durum_from_dashboard(d: &qtss_chart_patterns::SignalDashboardV1) -> Option<DashDurum> {
    match d.durum.as_str() {
        "LONG" => Some(DashDurum::Long),
        "SHORT" => Some(DashDurum::Short),
        "NOTR" => Some(DashDurum::Notr),
        _ => None,
    }
}

fn durum_transition_event_kinds(prev: Option<DashDurum>, new_d: DashDurum) -> Vec<&'static str> {
    match (prev, new_d) {
        // Önceki snapshot yok / `insufficient_bars` vb. → geçerli `durum` yok sayılır; ilk LONG/SHORT gözleminde giriş olayı.
        (None, DashDurum::Long) => vec!["long_entry"],
        (None, DashDurum::Short) => vec!["short_entry"],
        (None, DashDurum::Notr) => vec![],
        (Some(p), n) if p == n => vec![],
        (Some(DashDurum::Notr), DashDurum::Long) => vec!["long_entry"],
        (Some(DashDurum::Notr), DashDurum::Short) => vec!["short_entry"],
        (Some(DashDurum::Long), DashDurum::Notr) => vec!["long_exit"],
        (Some(DashDurum::Short), DashDurum::Notr) => vec!["short_exit"],
        (Some(DashDurum::Long), DashDurum::Short) => vec!["long_exit", "short_entry"],
        (Some(DashDurum::Short), DashDurum::Long) => vec!["short_exit", "long_entry"],
        _ => vec![],
    }
}

fn reference_price_for_signal(dash: &qtss_chart_patterns::SignalDashboardV1, last_close: f64) -> f64 {
    dash.giris_gercek
        .filter(|x| x.is_finite())
        .unwrap_or(last_close)
}

async fn record_range_signal_events_on_durum_change(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    prev_dash: Option<&serde_json::Value>,
    dash: &qtss_chart_patterns::SignalDashboardV1,
    bar_open_time: chrono::DateTime<chrono::Utc>,
    last_close: f64,
) {
    let Some(new_d) = durum_from_dashboard(dash) else {
        return;
    };
    let prev = prev_dash.and_then(durum_from_signal_payload);
    let kinds = durum_transition_event_kinds(prev, new_d);
    if kinds.is_empty() {
        return;
    }
    let px = reference_price_for_signal(dash, last_close);
    if !px.is_finite() {
        return;
    }
    for kind in kinds {
        let row = RangeSignalEventInsert {
            engine_symbol_id,
            event_kind: kind.to_string(),
            bar_open_time,
            reference_price: Some(px),
            source: "signal_dashboard_durum".into(),
            payload: json!({
                "prev_durum": prev.map(|d| d.as_str()),
                "new_durum": new_d.as_str(),
                "piyasa_modu": &dash.piyasa_modu,
                "yerel_trend": &dash.yerel_trend,
            }),
        };
        match insert_range_signal_event(pool, &row).await {
            Ok(Some(_)) => {
                info!(symbol_id = %engine_symbol_id, %kind, "range_signal_event");
            }
            Ok(None) => {}
            Err(e) => warn!(%e, %kind, "range_signal_event insert"),
        }
    }
}

async fn maybe_notify_sweep_transitions(
    bundle: Option<&(NotificationDispatcher, Vec<NotificationChannel>)>,
    t: &EngineSymbolRow,
    prev_payload: Option<&serde_json::Value>,
    tr: &TradingRangeResult,
) {
    let Some((d, chans)) = bundle else {
        return;
    };
    let policy = signal_direction_policy_for_row(t);
    let fire_long = tr.long_sweep_signal && policy != SignalDirectionPolicy::ShortOnly;
    let fire_short = tr.short_sweep_signal && policy != SignalDirectionPolicy::LongOnly;
    let (prev_l, prev_s) = prev_payload
        .map(sweep_flags_from_payload)
        .unwrap_or((false, false));
    if fire_long && !prev_l {
        notify_sweep_edge(d, chans, t, "L", tr).await;
    }
    if fire_short && !prev_s {
        notify_sweep_edge(d, chans, t, "S", tr).await;
    }
}

async fn run_engines_for_symbol(pool: &PgPool, bar_limit: i64, params: &TradingRangeParams) {
    let sweep_bundle = sweep_notify_bundle();
    let targets = match list_enabled_engine_symbols(pool).await {
        Ok(t) => t,
        Err(e) => {
            warn!(%e, "engine_symbols listesi");
            return;
        }
    };

    if targets.is_empty() {
        tracing::debug!("engine_symbols: etkin hedef yok");
        return;
    }

    for t in targets {
        let prev_tr_payload = match fetch_analysis_snapshot_payload(pool, t.id, "trading_range").await {
            Ok(p) => p,
            Err(e) => {
                warn!(%e, symbol = %t.symbol, "önceki trading_range payload");
                None
            }
        };
        let prev_dash_payload = match fetch_analysis_snapshot_payload(pool, t.id, "signal_dashboard").await {
            Ok(p) => p,
            Err(e) => {
                warn!(%e, symbol = %t.symbol, "önceki signal_dashboard payload");
                None
            }
        };

        let rows = match list_recent_bars(
            pool,
            &t.exchange,
            &t.segment,
            &t.symbol,
            &t.interval,
            bar_limit,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, symbol = %t.symbol, "market_bars okuma");
                continue;
            }
        };

        let need = params.lookback.max(5) + 2;
        if rows.len() < need {
            let err_payload = json!({"reason": "insufficient_bars", "need": need, "have": rows.len()});
            let _ = upsert_analysis_snapshot(
                pool,
                t.id,
                "trading_range",
                &err_payload,
                None,
                Some(rows.len() as i32),
                Some("yetersiz mum"),
            )
            .await;
            let _ = upsert_analysis_snapshot(
                pool,
                t.id,
                "signal_dashboard",
                &err_payload,
                None,
                Some(rows.len() as i32),
                Some("yetersiz mum"),
            )
            .await;
            continue;
        }

        let rows_chrono: Vec<_> = rows.into_iter().rev().collect();
        let n = rows_chrono.len();
        let lookback = params.lookback.max(5);
        let win_start = n - 1 - lookback;
        let win_end = n - 2;

        let bars: Vec<OhlcBar> = rows_chrono
            .iter()
            .enumerate()
            .map(|(i, r)| OhlcBar {
                open: d_to_f(r.open),
                high: d_to_f(r.high),
                low: d_to_f(r.low),
                close: d_to_f(r.close),
                bar_index: i as i64,
            })
            .collect();

        let tr = analyze_trading_range(&bars, params);
        let times: Vec<_> = rows_chrono.iter().map(|r| r.open_time).collect();
        let tr_payload = enrich_tr_payload(&tr, win_start, win_end, &times);
        let last_ot = rows_chrono.last().map(|r| r.open_time);
        let tr_err = if tr.bar_count > 0 && !tr.valid {
            tr.reason.clone().or_else(|| Some("geçersiz".into()))
        } else {
            None
        };

        if let Err(e) = upsert_analysis_snapshot(
            pool,
            t.id,
            "trading_range",
            &tr_payload,
            last_ot,
            Some(n as i32),
            tr_err.as_deref(),
        )
        .await
        {
            warn!(%e, symbol = %t.symbol, "trading_range snapshot");
        } else {
            info!(
                symbol = %t.symbol,
                interval = %t.interval,
                valid = tr.valid,
                "trading_range snapshot"
            );
            maybe_notify_sweep_transitions(sweep_bundle.as_ref(), &t, prev_tr_payload.as_ref(), &tr).await;
        }

        let direction_policy = signal_direction_policy_for_row(&t);
        let dash = compute_signal_dashboard_v1_with_policy(&bars, &tr, direction_policy);
        let dash_payload = enrich_dashboard_payload(
            &dash,
            &tr,
            rows_chrono.last().map(|r| r.open_time).unwrap_or_else(Utc::now),
            &t,
            direction_policy,
        );
        let last_bar_ot = rows_chrono.last().map(|r| r.open_time).unwrap_or_else(Utc::now);
        let last_close = rows_chrono
            .last()
            .and_then(|r| r.close.to_f64())
            .unwrap_or(f64::NAN);

        if let Err(e) = upsert_analysis_snapshot(
            pool,
            t.id,
            "signal_dashboard",
            &dash_payload,
            last_ot,
            Some(n as i32),
            None,
        )
        .await
        {
            warn!(%e, symbol = %t.symbol, "signal_dashboard snapshot");
        } else {
            info!(symbol = %t.symbol, interval = %t.interval, "signal_dashboard snapshot");
            record_range_signal_events_on_durum_change(
                pool,
                t.id,
                prev_dash_payload.as_ref(),
                &dash,
                last_bar_ot,
                last_close,
            )
            .await;
        }
    }
}

pub async fn engine_analysis_loop(pool: PgPool) {
    let secs: u64 = std::env::var("QTSS_ENGINE_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
        .max(15);

    let bar_limit: i64 = std::env::var("QTSS_ENGINE_BARS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_500)
        .clamp(120, 50_000);

    let params = trading_range_params_from_env();

    loop {
        run_engines_for_symbol(&pool, bar_limit, &params).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}
