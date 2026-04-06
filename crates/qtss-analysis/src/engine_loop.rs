//! `engine_symbols` rows → `trading_range` + `signal_dashboard` snapshots (see crate root).
//!
//! **Multi-symbol:** `run_engines_for_symbol` iterates every **enabled** `engine_symbols` row; each target loads its own
//! `market_bars` series and upserts both snapshots. Spawned from `qtss-worker` as `engine_analysis_loop` (single task, not one process per symbol).
//!
//! Optional sweep notify: `notify.notify_on_sweep` / `QTSS_NOTIFY_ON_SWEEP` + channel list (`notify_on_sweep_channels` / `QTSS_NOTIFY_ON_SWEEP_CHANNELS`). Same for range events: `notify_on_range_events` (+ channels). Credentials: `qtss_ai::load_notify_config_merged` (`dispatcher_config`, `telegram_*` rows, env when overrides on).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use super::ConfluencePersist;
use qtss_chart_patterns::{
    analyze_trading_range, classify_position_scenario, classify_score_trend,
    compute_signal_dashboard_v1_with_policy, roll_position_strength_history, scan_formations,
    signal_dashboard_v2_envelope_from_v1, zigzag_from_ohlc_bars, pivots_chronological,
    FormationParams, OhlcBar, PositionScenarioKind, SignalDirectionPolicy, TradingRangeParams,
    TradingRangeResult,
};
use qtss_ai::load_notify_config_merged;
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_common::{log_business, QtssLogLevel};
use qtss_storage::{
    clear_refresh_requested, default_range_engine_json, fetch_analysis_snapshot_payload,
    fetch_data_snapshot, fetch_latest_onchain_signal_score, fetch_range_engine_json,
    fetch_sibling_tbm_snapshots,
    insert_range_signal_event, list_enabled_engine_symbols, list_recent_bars,
    resolve_system_string, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    upsert_analysis_snapshot, EngineSymbolRow, RangeSignalEventInsert,
};
use rust_decimal::prelude::ToPrimitive;
use qtss_indicators::indicator_bundle::compute_all as compute_indicators;
use qtss_tbm::{
    score_tbm,
    setup::{detect_setups, SetupThresholds},
    mtf::{mtf_confirm, TfScore, Timeframe},
    scorer::TbmSignal,
};
use serde_json::json;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn d_to_f(d: rust_decimal::Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

/// İngilizce `snake_case` — payload içinde sembol bağlamı (JOIN’siz okuyan istemciler için).
fn attach_engine_context(t: &EngineSymbolRow, v: &mut serde_json::Value) {
    if let serde_json::Value::Object(ref mut m) = v {
        m.insert("symbol".into(), json!(t.symbol));
        m.insert("exchange".into(), json!(t.exchange));
        m.insert("segment".into(), json!(t.segment));
        m.insert("interval".into(), json!(t.interval));
        m.insert("engine_symbol_id".into(), json!(t.id.to_string()));
    }
}

/// `app_config.range_engine.trading_range_params` öncelikli; alan yoksa veya `null` ise `QTSS_TR_*` env.
fn trading_range_params_from_doc_and_env(doc: &JsonValue) -> TradingRangeParams {
    let tr = doc.get("trading_range_params");
    let lookback = tr
        .and_then(|o| o.get("lookback"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_LOOKBACK")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(50);
    let atr_period = tr
        .and_then(|o| o.get("atr_period"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_ATR_PERIOD")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(14);
    let atr_sma_period = tr
        .and_then(|o| o.get("atr_sma_period"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_ATR_SMA_PERIOD")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(50);
    let require_range_regime = tr
        .and_then(|o| o.get("require_range_regime"))
        .and_then(|x| x.as_bool())
        .unwrap_or_else(|| {
            std::env::var("QTSS_TR_REQUIRE_RANGE_REGIME")
                .ok()
                .is_some_and(|s| {
                    s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
                })
        });
    let pivot_window = tr
        .and_then(|o| o.get("pivot_window"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| std::env::var("QTSS_TR_PIVOT_WINDOW").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(3);
    let min_support_touches = tr
        .and_then(|o| o.get("min_support_touches"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_MIN_SUPPORT_TOUCHES")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(2);
    let min_resistance_touches = tr
        .and_then(|o| o.get("min_resistance_touches"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_MIN_RESISTANCE_TOUCHES")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(2);
    let touch_tolerance_atr_mult = tr
        .and_then(|o| o.get("touch_tolerance_atr_mult"))
        .and_then(|x| x.as_f64())
        .or_else(|| {
            std::env::var("QTSS_TR_TOUCH_TOLERANCE_ATR_MULT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0.25);
    let close_breakout_lookback = tr
        .and_then(|o| o.get("close_breakout_lookback"))
        .and_then(|x| x.as_u64())
        .map(|u| u as usize)
        .or_else(|| {
            std::env::var("QTSS_TR_CLOSE_BREAKOUT_LOOKBACK")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(20);
    let range_width_min_atr_mult = tr
        .and_then(|o| o.get("range_width_min_atr_mult"))
        .and_then(|x| x.as_f64())
        .or_else(|| {
            std::env::var("QTSS_TR_RANGE_WIDTH_MIN_ATR_MULT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(1.0);
    let range_width_max_atr_mult = tr
        .and_then(|o| o.get("range_width_max_atr_mult"))
        .and_then(|x| x.as_f64())
        .or_else(|| {
            std::env::var("QTSS_TR_RANGE_WIDTH_MAX_ATR_MULT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(6.0);
    let setup_score_threshold = tr
        .and_then(|o| o.get("setup_score_threshold"))
        .and_then(|x| x.as_i64())
        .map(|i| i as i32)
        .or_else(|| {
            std::env::var("QTSS_TR_SETUP_SCORE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(60);
    let setup_score_strong_threshold = tr
        .and_then(|o| o.get("setup_score_strong_threshold"))
        .and_then(|x| x.as_i64())
        .map(|i| i as i32)
        .or_else(|| {
            std::env::var("QTSS_TR_SETUP_SCORE_STRONG_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(75);
    let zone_edge_fraction = tr
        .and_then(|o| o.get("zone_edge_fraction"))
        .and_then(|x| x.as_f64())
        .or_else(|| {
            std::env::var("QTSS_TR_ZONE_EDGE_FRACTION")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0.25);
    let enable_range_zone_filter = tr
        .and_then(|o| o.get("enable_range_zone_filter"))
        .and_then(|x| x.as_bool())
        .unwrap_or_else(|| {
            std::env::var("QTSS_TR_ENABLE_RANGE_ZONE_FILTER")
                .ok()
                .map(|s| {
                    let t = s.trim().to_lowercase();
                    !(t == "0" || t == "false" || t == "no" || t == "off")
                })
                .unwrap_or(true)
        });
    let require_edge_reclaim_for_setup = tr
        .and_then(|o| o.get("require_edge_reclaim_for_setup"))
        .and_then(|x| x.as_bool())
        .unwrap_or_else(|| {
            std::env::var("QTSS_TR_REQUIRE_EDGE_RECLAIM")
                .ok()
                .map(|s| {
                    let t = s.trim().to_lowercase();
                    !(t == "0" || t == "false" || t == "no" || t == "off")
                })
                .unwrap_or(true)
        });
    TradingRangeParams {
        lookback,
        atr_period,
        atr_sma_period,
        require_range_regime,
        pivot_window,
        min_support_touches,
        min_resistance_touches,
        touch_tolerance_atr_mult,
        close_breakout_lookback,
        range_width_min_atr_mult,
        range_width_max_atr_mult,
        setup_score_threshold,
        setup_score_strong_threshold,
        zone_edge_fraction,
        enable_range_zone_filter,
        require_edge_reclaim_for_setup,
    }
}

#[derive(Debug, Clone, Copy)]
struct ExecutionGates {
    allow_long_open: bool,
    allow_short_open: bool,
    allow_all_closes: bool,
}

impl Default for ExecutionGates {
    fn default() -> Self {
        Self {
            allow_long_open: true,
            allow_short_open: true,
            allow_all_closes: true,
        }
    }
}

fn execution_gates_from_doc(doc: &JsonValue) -> ExecutionGates {
    let g = doc.get("execution_gates");
    let b = |key: &str, d: bool| {
        g.and_then(|o| o.get(key))
            .and_then(|x| x.as_bool())
            .unwrap_or(d)
    };
    ExecutionGates {
        allow_long_open: b("allow_long_open", true),
        allow_short_open: b("allow_short_open", true),
        allow_all_closes: b("allow_all_closes", true),
    }
}

fn log_if_event_gated(kind: &str, gates: ExecutionGates) {
    let gated = match kind {
        "long_entry" => !gates.allow_long_open,
        "short_entry" => !gates.allow_short_open,
        "long_exit" | "short_exit" => !gates.allow_all_closes,
        _ => false,
    };
    if gated {
        tracing::debug!(
            target: "qtss",
            qtss_module = "qtss_worker::range_execution",
            event_kind = %kind,
            allow_long_open = gates.allow_long_open,
            allow_short_open = gates.allow_short_open,
            allow_all_closes = gates.allow_all_closes,
            "execution gate: web/config ile kapatılmış; olaylar DB’de tutulur, otomatik emir katmanı ileride burayı okuyacak"
        );
    }
}

fn enrich_tr_payload(
    tr: &TradingRangeResult,
    window_start_idx: usize,
    window_end_idx: usize,
    chrono_open_times: &[chrono::DateTime<chrono::Utc>],
    t: &EngineSymbolRow,
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
        // Keep a stable subset of score fields at top-level for web overlay compatibility
        // even if serde omits future fields (explicit insert guards schema drift).
        m.insert("guardrails_pass".into(), json!(tr.guardrails_pass));
        m.insert("setup_side".into(), json!(tr.setup_side));
        m.insert("range_zone".into(), json!(tr.range_zone));
        m.insert("setup_score_long".into(), json!(tr.setup_score_long));
        m.insert("setup_score_short".into(), json!(tr.setup_score_short));
        m.insert("setup_score_best".into(), json!(tr.setup_score_best));
        m.insert("score_touch_long".into(), json!(tr.score_touch_long));
        m.insert("score_touch_short".into(), json!(tr.score_touch_short));
        m.insert("score_rejection_long".into(), json!(tr.score_rejection_long));
        m.insert("score_rejection_short".into(), json!(tr.score_rejection_short));
        m.insert("score_oscillator_long".into(), json!(tr.score_oscillator_long));
        m.insert("score_oscillator_short".into(), json!(tr.score_oscillator_short));
        m.insert("score_volume_long".into(), json!(tr.score_volume_long));
        m.insert("score_volume_short".into(), json!(tr.score_volume_short));
        m.insert("score_breakout_long".into(), json!(tr.score_breakout_long));
        m.insert("score_breakout_short".into(), json!(tr.score_breakout_short));
        m.insert("volume_unavailable".into(), json!(tr.volume_unavailable));
    }
    attach_engine_context(t, &mut v);
    v
}

fn signal_direction_policy_for_row(t: &EngineSymbolRow) -> SignalDirectionPolicy {
    match t.signal_direction_mode.trim().to_lowercase().as_str() {
        "both" | "bidirectional" | "long_short" | "long_and_short" => SignalDirectionPolicy::Both,
        "long_only" | "longonly" => SignalDirectionPolicy::LongOnly,
        "short_only" | "shortonly" => SignalDirectionPolicy::ShortOnly,
        "auto_segment" | "auto" | "" => {
            let seg = t.segment.trim().to_lowercase();
            if matches!(seg.as_str(), "futures" | "usdt_futures" | "fapi" | "future") {
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
            if matches!(seg.as_str(), "futures" | "usdt_futures" | "fapi" | "future") {
                SignalDirectionPolicy::Both
            } else {
                SignalDirectionPolicy::LongOnly
            }
        }
    }
}

/// TA yönü: +1 LONG, -1 SHORT, 0 NOTR / bilinmiyor.
fn ta_side_from_dashboard_json(dash: &serde_json::Value) -> i8 {
    if let Some(v2) = dash.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            if let Some(s) = v2.get("status").and_then(|x| x.as_str()) {
                match s.trim().to_uppercase().replace('İ', "I").as_str() {
                    "LONG" => return 1,
                    "SHORT" => return -1,
                    _ => {}
                }
            }
        }
    }
    match dash.get("durum").and_then(|x| x.as_str()) {
        Some("LONG") => 1,
        Some("SHORT") => -1,
        _ => 0,
    }
}

/// SPEC §6.2 — on-chain tablo + confluence snapshot; `pozisyon_gucu_10` hizalanmış yönle güçlendirilir, 0–12 → 0–10 normalize.
async fn merge_confluence_and_onchain_into_dashboard_json(
    pool: &PgPool,
    t: &EngineSymbolRow,
    dash: &mut serde_json::Value,
) {
    let conf = fetch_analysis_snapshot_payload(pool, t.id, "confluence")
        .await
        .unwrap_or(None);
    let onchain = fetch_latest_onchain_signal_score(pool, &t.symbol)
        .await
        .unwrap_or(None);

    let ta_side = ta_side_from_dashboard_json(dash);

    let mut delta: i32 = 0;
    if let Some(ref o) = onchain {
        if o.conflict_detected {
            delta -= 2;
        } else {
            let bullish = matches!(o.direction.as_str(), "strong_buy" | "buy");
            let bearish = matches!(o.direction.as_str(), "strong_sell" | "sell");
            let aligns = (ta_side == 1 && bullish) || (ta_side == -1 && bearish);
            if aligns {
                delta += match o.direction.as_str() {
                    "strong_buy" | "strong_sell" => 2,
                    "buy" | "sell" => 1,
                    _ => 0,
                };
            }
        }
    }

    let v1_strength = dash
        .get("pozisyon_gucu_10")
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .unwrap_or(5) as i32;
    let raw = (v1_strength + delta).clamp(0, 12);
    let norm = ((raw as f64 / 12.0) * 10.0).round() as u64;
    let norm_u8 = norm.min(10) as u8;

    if let Some(obj) = dash.as_object_mut() {
        obj.insert("pozisyon_gucu_10".into(), json!(norm_u8));
    }

    if let Some(v2) = dash
        .get_mut("signal_dashboard_v2")
        .and_then(|x| x.as_object_mut())
    {
        v2.insert("position_strength_10".into(), json!(norm_u8));
        if let Some(ref c) = conf {
            v2.insert(
                "confluence_brief".into(),
                json!({
                    "regime": c.get("regime"),
                    "composite_score": c.get("composite_score"),
                    "confidence_0_100": c.get("confidence_0_100"),
                    "lot_scale_hint": c.get("lot_scale_hint"),
                    "direction": c.get("direction"),
                    "conflicts_len": c.get("conflicts").and_then(|x| x.as_array()).map(|a| a.len()),
                }),
            );
        }
        if let Some(ref o) = onchain {
            v2.insert(
                "onchain_signal_brief".into(),
                json!({
                    "aggregate_score": o.aggregate_score,
                    "confidence": o.confidence,
                    "direction": o.direction,
                    "conflict_detected": o.conflict_detected,
                    "computed_at": o.computed_at,
                }),
            );
        }
    }
}

fn parse_position_strength_history_from_payload(v: &serde_json::Value) -> Option<Vec<u8>> {
    v.get("position_strength_history_10")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|el| el.as_u64().map(|u| u.min(10) as u8))
                .collect()
        })
}

fn dashboard_strength_u8(dash: &serde_json::Value) -> u8 {
    dash.get("pozisyon_gucu_10")
        .and_then(|x| {
            x.as_u64()
                .or_else(|| x.as_i64().map(|i| i.clamp(0, 10) as u64))
        })
        .map(|u| u.min(10) as u8)
        .unwrap_or(5)
}

fn ascii_upper_durum(s: &str) -> String {
    s.trim().to_uppercase().replace('İ', "I")
}

fn compute_entry_strength_for_payload(
    prev: Option<&serde_json::Value>,
    prev_dur: Option<&str>,
    new_dur: &str,
    current_strength: u8,
) -> Option<u8> {
    let nd = ascii_upper_durum(new_dur);
    let long_short = nd == "LONG" || nd == "SHORT";
    if !long_short {
        return None;
    }
    let prev_notr = prev_dur
        .map(|d| {
            let u = ascii_upper_durum(d);
            u == "NOTR" || u.is_empty()
        })
        .unwrap_or(true);
    let prev_entry = prev
        .and_then(|p| p.get("position_strength_entry_10"))
        .and_then(|x| x.as_u64())
        .map(|u| u.min(10) as u8);

    if prev_notr || prev_entry.is_none() {
        Some(current_strength)
    } else {
        prev_entry
    }
}

/// `docs/SIGNAL_POSITION_SCORE_RULES.md` — rolling strength history, trend/scenario hints (after confluence merge).
fn attach_position_score_signal_fields(
    dash: &mut serde_json::Value,
    prev: Option<&serde_json::Value>,
) {
    if dash.get("reason").and_then(|x| x.as_str()) == Some("insufficient_bars") {
        return;
    }
    let current_strength = dashboard_strength_u8(dash);
    let prev_hist = prev.and_then(parse_position_strength_history_from_payload);
    let history = roll_position_strength_history(prev_hist.as_deref(), current_strength);
    let trend = classify_score_trend(&history);

    let prev_dur = prev
        .and_then(|p| p.get("durum"))
        .and_then(|x| x.as_str());
    let new_dur = dash.get("durum").and_then(|x| x.as_str()).unwrap_or("NOTR");
    let entry_opt = compute_entry_strength_for_payload(prev, prev_dur, new_dur, current_strength);

    let scenario = entry_opt
        .map(|e| classify_position_scenario(e, current_strength))
        .unwrap_or(PositionScenarioKind::None);

    if let Some(obj) = dash.as_object_mut() {
        obj.insert("position_strength_history_10".into(), json!(history));
        obj.insert("score_trend_kind".into(), json!(trend.kind.as_str()));
        obj.insert("score_trend_action".into(), json!(trend.action));
        if let Some(e) = entry_opt {
            obj.insert("position_strength_entry_10".into(), json!(e));
        } else {
            obj.remove("position_strength_entry_10");
        }
        obj.insert("position_scenario_kind".into(), json!(scenario.as_str()));
    }

    if let Some(v2) = dash
        .get_mut("signal_dashboard_v2")
        .and_then(|x| x.as_object_mut())
    {
        v2.insert("position_strength_history_10".into(), json!(&history));
        v2.insert("score_trend_kind".into(), json!(trend.kind.as_str()));
        v2.insert("score_trend_action".into(), json!(trend.action));
        if let Some(e) = entry_opt {
            v2.insert("position_strength_entry_10".into(), json!(e));
        } else {
            v2.remove("position_strength_entry_10");
        }
        v2.insert("position_scenario_kind".into(), json!(scenario.as_str()));
    }
}

fn json_finite_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

fn signal_levels_ready_json(d: &serde_json::Value) -> bool {
    d.get("giris_gercek").and_then(json_finite_f64).is_some()
        && d.get("stop_ilk").and_then(json_finite_f64).is_some()
        && d.get("kar_al_ilk").and_then(json_finite_f64).is_some()
}

/// LONG/SHORT ve üçlü seviye varken giriş fiyatını kurulum anına sabitler (`setup_entry_price`); NOTR veya seviye yokken alan kaldırılır.
fn attach_setup_entry_price(
    dash: &mut serde_json::Value,
    prev: Option<&serde_json::Value>,
    last_close: f64,
) {
    if dash.get("reason").and_then(|x| x.as_str()) == Some("insufficient_bars") {
        return;
    }
    let new_dur = dash.get("durum").and_then(|x| x.as_str()).unwrap_or("NOTR");
    let nd = ascii_upper_durum(new_dur);
    let active_setup = (nd == "LONG" || nd == "SHORT") && signal_levels_ready_json(dash);

    if !active_setup {
        if let Some(obj) = dash.as_object_mut() {
            obj.remove("setup_entry_price");
        }
        if let Some(v2) = dash
            .get_mut("signal_dashboard_v2")
            .and_then(|x| x.as_object_mut())
        {
            v2.remove("setup_entry_price");
        }
        return;
    }

    let prev_dur_s = prev
        .and_then(|p| p.get("durum"))
        .and_then(|x| x.as_str());
    let pd = prev_dur_s.map(ascii_upper_durum);
    let prev_notr = pd
        .as_ref()
        .map_or(true, |d| d == "NOTR" || d.is_empty());
    let prev_same_side = pd.as_ref().map(|d| *d == nd).unwrap_or(false);

    let engine_giris = dash
        .get("giris_gercek")
        .and_then(json_finite_f64)
        .unwrap_or(last_close);

    let prev_frozen = prev
        .and_then(|p| p.get("setup_entry_price"))
        .and_then(json_finite_f64);
    let prev_giris = prev
        .and_then(|p| p.get("giris_gercek"))
        .and_then(json_finite_f64);

    let frozen = if prev_notr || !prev_same_side {
        engine_giris
    } else {
        prev_frozen
            .or(prev_giris)
            .unwrap_or(engine_giris)
    };

    if let Some(obj) = dash.as_object_mut() {
        obj.insert("setup_entry_price".into(), json!(frozen));
        obj.insert("giris_gercek".into(), json!(frozen));
    }
    if let Some(v2) = dash
        .get_mut("signal_dashboard_v2")
        .and_then(|x| x.as_object_mut())
    {
        v2.insert("setup_entry_price".into(), json!(frozen));
        v2.insert("entry_price".into(), json!(frozen));
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
        m.insert(
            "last_bar_open_time".into(),
            json!(last_open_time.to_rfc3339()),
        );
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
        // Grafik likidite süpürme işaretleri: `trading_range` snapshot’ı yoksa bile `signal_dashboard` yükünden okunabilir.
        m.insert("long_sweep_latent".into(), json!(tr.long_sweep_latent));
        m.insert("short_sweep_latent".into(), json!(tr.short_sweep_latent));
        m.insert("long_sweep_signal".into(), json!(tr.long_sweep_signal));
        m.insert("short_sweep_signal".into(), json!(tr.short_sweep_signal));
        // Skor tabanlı range setup motoru metrikleri (UI/SSS için).
        m.insert("support_touches".into(), json!(tr.support_touches));
        m.insert("resistance_touches".into(), json!(tr.resistance_touches));
        m.insert("close_breakout".into(), json!(tr.close_breakout));
        if let Some(x) = tr.range_width {
            m.insert("range_width".into(), json!(x));
        }
        if let Some(x) = tr.range_width_atr {
            m.insert("range_width_atr".into(), json!(x));
        }
        m.insert("range_too_narrow".into(), json!(tr.range_too_narrow));
        m.insert("range_too_wide".into(), json!(tr.range_too_wide));
        m.insert("wick_rejection_long".into(), json!(tr.wick_rejection_long));
        m.insert("wick_rejection_short".into(), json!(tr.wick_rejection_short));
        m.insert("fake_breakout_long".into(), json!(tr.fake_breakout_long));
        m.insert("fake_breakout_short".into(), json!(tr.fake_breakout_short));
        m.insert("setup_score_long".into(), json!(tr.setup_score_long));
        m.insert("setup_score_short".into(), json!(tr.setup_score_short));
        m.insert("setup_score_best".into(), json!(tr.setup_score_best));
        m.insert("guardrails_pass".into(), json!(tr.guardrails_pass));
        m.insert("setup_side".into(), json!(tr.setup_side));
        m.insert("range_zone".into(), json!(tr.range_zone));
        m.insert("score_touch_long".into(), json!(tr.score_touch_long));
        m.insert("score_touch_short".into(), json!(tr.score_touch_short));
        m.insert("score_rejection_long".into(), json!(tr.score_rejection_long));
        m.insert("score_rejection_short".into(), json!(tr.score_rejection_short));
        m.insert("score_oscillator_long".into(), json!(tr.score_oscillator_long));
        m.insert("score_oscillator_short".into(), json!(tr.score_oscillator_short));
        m.insert("score_volume_long".into(), json!(tr.score_volume_long));
        m.insert("score_volume_short".into(), json!(tr.score_volume_short));
        m.insert("score_breakout_long".into(), json!(tr.score_breakout_long));
        m.insert("score_breakout_short".into(), json!(tr.score_breakout_short));
        m.insert("volume_unavailable".into(), json!(tr.volume_unavailable));
        m.insert(
            "signal_direction_mode".into(),
            json!(t.signal_direction_mode.as_str()),
        );
        m.insert(
            "signal_direction_effective".into(),
            json!(effective.as_api_str()),
        );
        if let Ok(v2) = serde_json::to_value(signal_dashboard_v2_envelope_from_v1(dash)) {
            m.insert("signal_dashboard_v2".into(), v2);
        }
    }
    attach_engine_context(t, &mut v);
    v
}

async fn sweep_notify_bundle(
    pool: &PgPool,
) -> Option<(NotificationDispatcher, Vec<NotificationChannel>)> {
    let enabled = resolve_worker_enabled_flag(
        pool,
        "notify",
        "notify_on_sweep",
        "QTSS_NOTIFY_ON_SWEEP",
        false,
    )
    .await;
    if !enabled {
        return None;
    }
    let raw = resolve_system_string(
        pool,
        "notify",
        "notify_on_sweep_channels",
        "QTSS_NOTIFY_ON_SWEEP_CHANNELS",
        "webhook",
    )
    .await;
    let chans: Vec<NotificationChannel> = raw
        .split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect();
    if chans.is_empty() {
        warn!("notify_on_sweep enabled but notify_on_sweep_channels is empty or invalid");
        return None;
    }
    let ncfg = load_notify_config_merged(pool).await;
    Some((NotificationDispatcher::new(ncfg), chans))
}

/// Trading Range **setup** (`giris_modu` → DÖNÜŞ) ve **işlem aç/kapa** (`range_signal_events`) için Telegram vb.
async fn range_events_notify_bundle(
    pool: &PgPool,
) -> Option<(NotificationDispatcher, Vec<NotificationChannel>)> {
    let enabled = resolve_worker_enabled_flag(
        pool,
        "notify",
        "notify_on_range_events",
        "QTSS_NOTIFY_ON_RANGE_EVENTS",
        false,
    )
    .await;
    if !enabled {
        return None;
    }
    let raw = resolve_system_string(
        pool,
        "notify",
        "notify_on_range_events_channels",
        "QTSS_NOTIFY_ON_RANGE_EVENTS_CHANNELS",
        "telegram",
    )
    .await;
    let chans: Vec<NotificationChannel> = raw
        .split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect();
    if chans.is_empty() {
        warn!(
            "notify_on_range_events enabled but notify_on_range_events_channels is empty or invalid"
        );
        return None;
    }
    let ncfg = load_notify_config_merged(pool).await;
    Some((NotificationDispatcher::new(ncfg), chans))
}

async fn tbm_notify_bundle(
    pool: &PgPool,
) -> Option<(NotificationDispatcher, Vec<NotificationChannel>)> {
    let enabled = resolve_worker_enabled_flag(
        pool, "notify", "notify_on_tbm_setup", "QTSS_NOTIFY_ON_TBM_SETUP", false,
    ).await;
    if !enabled { return None; }
    let raw = resolve_system_string(
        pool, "notify", "notify_on_tbm_channels", "QTSS_NOTIFY_ON_TBM_CHANNELS", "telegram",
    ).await;
    let chans: Vec<NotificationChannel> = raw
        .split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect();
    if chans.is_empty() {
        warn!("notify_on_tbm_setup enabled but channels empty");
        return None;
    }
    let ncfg = load_notify_config_merged(pool).await;
    Some((NotificationDispatcher::new(ncfg), chans))
}

async fn notify_tbm_setup(
    d: &NotificationDispatcher,
    channels: &[NotificationChannel],
    t: &EngineSymbolRow,
    setup: &qtss_tbm::setup::TbmSetup,
) {
    let dir_emoji = match setup.direction {
        qtss_tbm::setup::SetupDirection::Bottom => "🟢",
        qtss_tbm::setup::SetupDirection::Top => "🔴",
    };
    let title = format!("{dir_emoji} TBM {:?} — {} {}", setup.direction, t.symbol, t.interval);
    let pillar_lines: String = setup.pillar_details.iter().take(8).map(|d| format!("• {d}")).collect::<Vec<_>>().join("\n");
    let body = format!(
        "Score: {:.1} | Signal: {:?}\n{}/{}\n\n{pillar_lines}",
        setup.score, setup.signal, t.exchange, t.segment,
    );
    let n = Notification::new(title, body);
    for r in d.send_all(channels, &n).await {
        if r.ok {
            info!(channel = ?r.channel, symbol = %t.symbol, "TBM setup bildirimi gönderildi");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, "TBM setup bildirimi başarısız");
        }
    }
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

/// `signal_dashboard` DONUS kenarı: likidite süpürmesi / dönüş modu (önceki yükte yoksa veya takipteydi).
async fn maybe_notify_trading_range_setup(
    bundle: Option<&(NotificationDispatcher, Vec<NotificationChannel>)>,
    prev_dash: Option<&serde_json::Value>,
    dash: &qtss_chart_patterns::SignalDashboardV1,
    tr: &TradingRangeResult,
    t: &EngineSymbolRow,
) {
    let Some((d, chans)) = bundle else {
        return;
    };
    if dash.giris_modu != "DONUS" {
        return;
    }
    let prev_ok = prev_dash.filter(|p| {
        p.get("reason").and_then(|x| x.as_str()) != Some("insufficient_bars")
    });
    let prev_giris = prev_ok.and_then(|p| p.get("giris_modu").and_then(|x| x.as_str()));
    if prev_giris == Some("DONUS") {
        return;
    }
    let title = format!("Trading Range setup — {} {}", t.symbol, t.interval);
    let body = format!(
        "{}/ {}\nGiriş modu: {} (DÖNÜŞ)\nPiyasa modu: {}\nYerel trend: {}\nDurum: {}\nAralık üst/alt: {:?} / {:?}\nSweep sinyal L/S: {} / {}",
        t.exchange,
        t.segment,
        dash.giris_modu,
        dash.piyasa_modu,
        dash.yerel_trend,
        dash.durum,
        tr.range_high,
        tr.range_low,
        tr.long_sweep_signal,
        tr.short_sweep_signal,
    );
    let n = Notification::new(title, body);
    for r in d.send_all(chans, &n).await {
        if r.ok {
            info!(channel = ?r.channel, symbol = %t.symbol, "trading_range_setup bildirimi");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, "trading_range_setup bildirimi başarısız");
        }
    }
}

async fn notify_range_trade_event(
    d: &NotificationDispatcher,
    channels: &[NotificationChannel],
    t: &EngineSymbolRow,
    event_kind: &str,
    bar_open_time: chrono::DateTime<chrono::Utc>,
    reference_price: f64,
    prev_durum: Option<&str>,
    new_durum: &str,
) {
    let title_label = match event_kind {
        "long_entry" => "LONG giriş",
        "short_entry" => "SHORT giriş",
        "long_exit" => "LONG kapanış",
        "short_exit" => "SHORT kapanış",
        _ => "Range işlem olayı",
    };
    let title = format!("{} — {} {}", title_label, t.symbol, t.interval);
    let prev_s = prev_durum.unwrap_or("—");
    let body = format!(
        "{}/ {}\nOlay: {}\nBar: {}\nReferans fiyat: {:.6}\nÖnceki durum: {}\nYeni durum: {}",
        t.exchange,
        t.segment,
        event_kind,
        bar_open_time.to_rfc3339(),
        reference_price,
        prev_s,
        new_durum,
    );
    let n = Notification::new(title, body);
    for r in d.send_all(channels, &n).await {
        if r.ok {
            info!(
                channel = ?r.channel,
                symbol = %t.symbol,
                event_kind = %event_kind,
                "range_trade_event bildirimi"
            );
        } else {
            warn!(
                channel = ?r.channel,
                detail = ?r.detail,
                event_kind = %event_kind,
                "range_trade_event bildirimi başarısız"
            );
        }
    }
}

async fn notify_range_setup_scored(
    d: &NotificationDispatcher,
    channels: &[NotificationChannel],
    t: &EngineSymbolRow,
    tr: &qtss_chart_patterns::TradingRangeResult,
    bar_open_time: chrono::DateTime<chrono::Utc>,
    reference_price: f64,
) {
    let side = tr.setup_side.trim().to_uppercase().replace('İ', "I");
    if side != "LONG" && side != "SHORT" {
        return;
    }
    if !tr.guardrails_pass {
        return;
    }
    let title = format!(
        "Trading Range setup — {} {} ({})",
        side,
        t.symbol,
        t.interval
    );
    let rh = tr.range_high.map(|x| format!("{x:.6}")).unwrap_or_else(|| "—".into());
    let rl = tr.range_low.map(|x| format!("{x:.6}")).unwrap_or_else(|| "—".into());
    let mid = tr.mid.map(|x| format!("{x:.6}")).unwrap_or_else(|| "—".into());
    let w_atr = tr
        .range_width_atr
        .map(|x| format!("{x:.2}"))
        .unwrap_or_else(|| "—".into());
    let body = format!(
        "{}/ {}\nBar: {}\nRef px: {:.6}\nBand: low={} · mid={} · high={}\nTouches: sup={} · res={}\nClose breakout: {}\nWidth/ATR: {}\nScore(best): {}\nBreakdown (L): touch={} rej={} osc={} vol={} brk={}\nFlags: wick(L/S)={} / {} · fake(L/S)={} / {}",
        t.exchange,
        t.segment,
        bar_open_time.to_rfc3339(),
        reference_price,
        rl,
        mid,
        rh,
        tr.support_touches,
        tr.resistance_touches,
        if tr.close_breakout { "yes" } else { "no" },
        w_atr,
        tr.setup_score_best,
        tr.score_touch_long,
        tr.score_rejection_long,
        tr.score_oscillator_long,
        tr.score_volume_long,
        tr.score_breakout_long,
        tr.wick_rejection_long,
        tr.wick_rejection_short,
        tr.fake_breakout_long,
        tr.fake_breakout_short,
    );
    let n = Notification::new(title, body);
    for r in d.send_all(channels, &n).await {
        if r.ok {
            info!(channel = ?r.channel, symbol = %t.symbol, "trading_range_setup scored bildirimi");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, "trading_range_setup scored bildirimi başarısız");
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

fn reference_price_for_signal(
    dash: &qtss_chart_patterns::SignalDashboardV1,
    last_close: f64,
) -> f64 {
    dash.giris_gercek
        .filter(|x| x.is_finite())
        .unwrap_or(last_close)
}

async fn record_range_signal_events_on_durum_change(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    t: &EngineSymbolRow,
    tr: &qtss_chart_patterns::TradingRangeResult,
    prev_dash: Option<&serde_json::Value>,
    dash: &qtss_chart_patterns::SignalDashboardV1,
    bar_open_time: chrono::DateTime<chrono::Utc>,
    last_close: f64,
    range_notify: Option<&(NotificationDispatcher, Vec<NotificationChannel>)>,
    gates: ExecutionGates,
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
    let prev_durum_str = prev.map(|d| d.as_str());
    let new_durum_str = new_d.as_str();
    for kind in kinds {
        log_if_event_gated(kind, gates);
        let kind_s = kind.to_string();
        let row = RangeSignalEventInsert {
            engine_symbol_id,
            event_kind: kind_s.clone(),
            bar_open_time,
            reference_price: Some(px),
            source: "signal_dashboard_durum".into(),
            payload: json!({
                "prev_durum": prev_durum_str,
                "new_durum": new_durum_str,
                "piyasa_modu": &dash.piyasa_modu,
                "yerel_trend": &dash.yerel_trend,
                "giris_gercek": dash.giris_gercek,
                "stop_ilk": dash.stop_ilk,
                "kar_al_ilk": dash.kar_al_ilk,
            }),
        };
        match insert_range_signal_event(pool, &row).await {
            Ok(Some(_)) => {
                info!(symbol_id = %engine_symbol_id, kind = %kind_s, "range_signal_event");
                if let Some((d, ch)) = range_notify {
                    if kind_s == "long_entry" || kind_s == "short_entry" {
                        notify_range_setup_scored(d, ch, t, tr, bar_open_time, px).await;
                    }
                    notify_range_trade_event(d, ch, t, &kind_s, bar_open_time, px, prev_durum_str, new_durum_str)
                        .await;
                }
            }
            Ok(None) => {}
            Err(e) => warn!(%e, kind = %kind_s, "range_signal_event insert"),
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

async fn run_engines_for_symbol(
    pool: &PgPool,
    bar_limit: i64,
    params: &TradingRangeParams,
    confluence: &Arc<dyn ConfluencePersist>,
    range_doc: &JsonValue,
) {
    let gates = execution_gates_from_doc(range_doc);
    let sweep_bundle = sweep_notify_bundle(pool).await;
    let range_events_bundle = range_events_notify_bundle(pool).await;
    let tbm_bundle = tbm_notify_bundle(pool).await;

    // TBM → Execution Bridge config
    let tbm_auto_execute = resolve_worker_enabled_flag(
        pool, "worker", "tbm_auto_execute_enabled", "QTSS_TBM_AUTO_EXECUTE_ENABLED", false,
    ).await;
    let tbm_execute_min_signal: u8 = {
        let s = resolve_system_string(
            pool, "worker", "tbm_execute_min_signal", "QTSS_TBM_EXECUTE_MIN_SIGNAL", "Strong",
        ).await;
        match s.as_str() {
            "VeryStrong" => 5,
            "Strong" => 4,
            "Moderate" => 3,
            "Weak" => 2,
            _ => 4, // default Strong
        }
    };
    if tbm_auto_execute {
        info!(min_signal = tbm_execute_min_signal, "TBM auto-execute aktif");
    }

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
        let prev_tr_payload =
            match fetch_analysis_snapshot_payload(pool, t.id, "trading_range").await {
                Ok(p) => p,
                Err(e) => {
                    warn!(%e, symbol = %t.symbol, "önceki trading_range payload");
                    None
                }
            };
        let prev_dash_payload =
            match fetch_analysis_snapshot_payload(pool, t.id, "signal_dashboard").await {
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
            let mut err_payload =
                json!({"reason": "insufficient_bars", "need": need, "have": rows.len()});
            attach_engine_context(&t, &mut err_payload);
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
                volume: Some(d_to_f(r.volume)),
            })
            .collect();

        let tr = analyze_trading_range(&bars, params);
        let times: Vec<_> = rows_chrono.iter().map(|r| r.open_time).collect();
        let tr_payload = enrich_tr_payload(&tr, win_start, win_end, &times, &t);
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
            maybe_notify_sweep_transitions(
                sweep_bundle.as_ref(),
                &t,
                prev_tr_payload.as_ref(),
                &tr,
            )
            .await;
        }

        let direction_policy = signal_direction_policy_for_row(&t);
        let dash = compute_signal_dashboard_v1_with_policy(&bars, &tr, direction_policy);
        maybe_notify_trading_range_setup(
            range_events_bundle.as_ref(),
            prev_dash_payload.as_ref(),
            &dash,
            &tr,
            &t,
        )
        .await;
        let mut dash_payload = enrich_dashboard_payload(
            &dash,
            &tr,
            rows_chrono
                .last()
                .map(|r| r.open_time)
                .unwrap_or_else(Utc::now),
            &t,
            direction_policy,
        );
        let last_bar_ot = rows_chrono
            .last()
            .map(|r| r.open_time)
            .unwrap_or_else(Utc::now);
        let last_close = rows_chrono
            .last()
            .and_then(|r| r.close.to_f64())
            .unwrap_or(f64::NAN);

        if let Err(e) = confluence
            .compute_and_persist(pool, &t, &dash_payload, last_bar_ot, n as i32)
            .await
        {
            warn!(%e, symbol = %t.symbol, "confluence snapshot");
        }

        merge_confluence_and_onchain_into_dashboard_json(pool, &t, &mut dash_payload).await;

        attach_position_score_signal_fields(&mut dash_payload, prev_dash_payload.as_ref());
        attach_setup_entry_price(
            &mut dash_payload,
            prev_dash_payload.as_ref(),
            last_close,
        );

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
                &t,
                &tr,
                prev_dash_payload.as_ref(),
                &dash,
                last_bar_ot,
                last_close,
                range_events_bundle.as_ref(),
                gates,
            )
            .await;
        }

        // ── Faz 2+3: Klasik formasyonlar (Double Top/Bottom, H&S, Triple, Flag) ──
        {
            let bar_map: std::collections::BTreeMap<i64, OhlcBar> =
                bars.iter().map(|b| (b.bar_index, *b)).collect();
            let zz = zigzag_from_ohlc_bars(&bar_map, 8, 50, 0);
            let chrono = pivots_chronological(&zz);
            let pivot_triples: Vec<(i64, f64, i32)> = chrono
                .iter()
                .map(|p| (p.point.index, p.point.price, p.dir))
                .collect();
            let formations = scan_formations(&pivot_triples, &bars, &FormationParams::default());
            if !formations.is_empty() {
                let payload = json!({
                    "formations": formations,
                    "pivot_count": pivot_triples.len(),
                    "bar_count": bars.len(),
                });
                if let Err(e) = upsert_analysis_snapshot(
                    pool,
                    t.id,
                    "formations",
                    &payload,
                    last_ot,
                    Some(n as i32),
                    None,
                )
                .await
                {
                    warn!(%e, symbol = %t.symbol, "formations snapshot");
                } else {
                    info!(
                        symbol = %t.symbol,
                        count = formations.len(),
                        "formations snapshot"
                    );
                }
            }
        }

        // ── Faz C: TBM (Top/Bottom Mining) skorlama ──
        {
            let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
            let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
            let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();
            let opens: Vec<f64> = bars.iter().map(|b| b.open).collect();
            let volumes: Vec<f64> = bars.iter().map(|b| b.volume.unwrap_or(0.0)).collect();

            let bundle = compute_indicators(&opens, &highs, &lows, &closes, &volumes, &[]);

            // Faz E: Onchain veri çekme
            let onchain_metrics = {
                let row = fetch_latest_onchain_signal_score(pool, &t.symbol).await.unwrap_or(None);
                if let Some(r) = row {
                    // meta_json'dan whale tx count çıkar
                    let whale_count = r.meta_json.as_ref()
                        .and_then(|m| m.get("source_breakdown"))
                        .and_then(|sb| sb.get("nansen_whale_perp_aggregate"))
                        .and_then(|w| w.get("trade_count"))
                        .and_then(|c| c.as_u64())
                        .map(|c| c as u32);

                    // Bireysel skorlardan veya aggregate+direction'dan metric oluştur
                    let smart_money = r.nansen_netflow_score
                        .or(r.nansen_sm_score)
                        .map(|s| -s * 1000.0);

                    let exchange_nf = r.exchange_netflow_score
                        .or_else(|| {
                            // Coinglass/HL verisi yoksa aggregate_score + direction'dan türet
                            if r.aggregate_score.abs() > 0.001 {
                                Some(-r.aggregate_score)
                            } else {
                                None
                            }
                        })
                        .map(|s| s * 1000.0);

                    let funding = r.funding_score
                        .or(r.oi_score) // OI skoru funding proxy olabilir
                        .map(|s| s * 0.05);

                    // HL whale + nansen perp skorlarından whale aktivite tahmini
                    let whale_est = whale_count.or_else(|| {
                        let wh = r.hl_whale_score.unwrap_or(0.0).abs()
                            + r.nansen_perp_score.unwrap_or(0.0).abs();
                        if wh > 0.01 { Some((wh * 100.0) as u32) } else { None }
                    });

                    qtss_tbm::onchain::OnchainMetrics {
                        smart_money_net_flow: smart_money,
                        exchange_netflow: exchange_nf,
                        whale_tx_count: whale_est,
                        funding_rate: funding,
                    }
                } else {
                    // Onchain skor satırı yok — DefiLlama stablecoin flow'dan proxy oluştur
                    let stable_flow = fetch_data_snapshot(pool, "defillama_stablecoin_flow")
                        .await
                        .ok()
                        .flatten()
                        .and_then(|s| s.response_json)
                        .and_then(|j| j.get("stablecoin_flow_pct").and_then(|v| v.as_f64()));

                    if let Some(flow_pct) = stable_flow {
                        // Stablecoin artışı → risk appetite yüksek → exchange'e giriş olabilir
                        qtss_tbm::onchain::OnchainMetrics {
                            smart_money_net_flow: None,
                            exchange_netflow: Some(-flow_pct * 100.0), // pozitif flow = bullish
                            whale_tx_count: None,
                            funding_rate: None,
                        }
                    } else {
                        qtss_tbm::onchain::OnchainMetrics::default()
                    }
                }
            };

            let last = closes.len().saturating_sub(1);
            let prev = last.saturating_sub(1);

            // Zigzag pivotlardan high/low pivot listesi oluştur
            let bar_map: std::collections::BTreeMap<i64, OhlcBar> =
                bars.iter().map(|b| (b.bar_index, *b)).collect();
            let zz = zigzag_from_ohlc_bars(&bar_map, 8, 50, 0);
            let chrono = pivots_chronological(&zz);
            let (mut price_highs, mut price_lows): (Vec<(usize, f64)>, Vec<(usize, f64)>) = (vec![], vec![]);
            let (mut macd_highs, mut macd_lows): (Vec<(usize, f64)>, Vec<(usize, f64)>) = (vec![], vec![]);
            for p in &chrono {
                let idx = p.point.index as usize;
                if idx < bundle.macd.macd_line.len() && !bundle.macd.macd_line[idx].is_nan() {
                    if p.dir > 0 {
                        price_highs.push((idx, p.point.price));
                        macd_highs.push((idx, bundle.macd.macd_line[idx]));
                    } else {
                        price_lows.push((idx, p.point.price));
                        macd_lows.push((idx, bundle.macd.macd_line[idx]));
                    }
                }
            }

            // En güçlü formasyon
            let pivot_triples: Vec<(i64, f64, i32)> = chrono
                .iter()
                .map(|p| (p.point.index, p.point.price, p.dir))
                .collect();
            let formations = scan_formations(&pivot_triples, &bars, &FormationParams::default());
            let best_formation = formations.iter().max_by(|a, b| a.quality.partial_cmp(&b.quality).unwrap_or(std::cmp::Ordering::Equal));
            let (form_quality, form_name) = best_formation.map(|f| (f.quality, f.pattern_name)).unwrap_or((0.0, ""));

            let get_val = |v: &[f64], i: usize| -> f64 {
                if i < v.len() && !v[i].is_nan() { v[i] } else { 0.0 }
            };

            // OBV ve CVD eğimi (son 10 bar)
            let slope = |series: &[f64], lookback: usize| -> f64 {
                if series.len() < lookback + 1 { return 0.0; }
                let end = series.len() - 1;
                let start = end - lookback;
                series[end] - series[start]
            };

            // Fibonacci proximity
            let fib_proximity = {
                let swing_high = highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let swing_low = lows.iter().cloned().fold(f64::INFINITY, f64::min);
                let levels = qtss_indicators::fibonacci::fib_retracements(swing_high, swing_low);
                let price = closes[last];
                let range = (swing_high - swing_low).max(1e-10);
                levels.iter().map(|l| 1.0 - ((price - l.price).abs() / range).min(1.0)).fold(0.0_f64, f64::max)
            };

            // Bottom score
            let bottom_pillars = vec![
                qtss_tbm::momentum::score_momentum(
                    get_val(&bundle.stochastic.k, last),
                    get_val(&bundle.stochastic.d, last),
                    get_val(&bundle.macd.histogram, last),
                    get_val(&bundle.macd.histogram, prev),
                    get_val(&bundle.ema_9, last),
                    get_val(&bundle.ema_21, last),
                    &price_highs, &price_lows, &macd_highs, &macd_lows,
                    true,
                ),
                qtss_tbm::volume::score_volume(
                    get_val(&bundle.mfi_14, last),
                    slope(&bundle.obv, 10),
                    slope(&bundle.cvd, 10),
                    volumes.get(last).copied().unwrap_or(0.0),
                    get_val(&bundle.sma_20, last),
                    true,
                ),
                qtss_tbm::structure::score_structure(
                    fib_proximity, "nearest",
                    get_val(&bundle.bollinger.percent_b, last),
                    qtss_indicators::volatility::bb_squeeze(&bundle.bollinger.bandwidth, 0.03).get(last).copied().unwrap_or(false),
                    qtss_indicators::volatility::compression_detector(&bundle.atr_14, 10).get(last).copied().unwrap_or(false),
                    form_quality, form_name,
                    true,
                ),
                qtss_tbm::onchain::score_onchain(&onchain_metrics, true),
            ];
            let bottom_score = score_tbm(bottom_pillars);

            // Top score
            let top_pillars = vec![
                qtss_tbm::momentum::score_momentum(
                    get_val(&bundle.stochastic.k, last),
                    get_val(&bundle.stochastic.d, last),
                    get_val(&bundle.macd.histogram, last),
                    get_val(&bundle.macd.histogram, prev),
                    get_val(&bundle.ema_9, last),
                    get_val(&bundle.ema_21, last),
                    &price_highs, &price_lows, &macd_highs, &macd_lows,
                    false,
                ),
                qtss_tbm::volume::score_volume(
                    get_val(&bundle.mfi_14, last),
                    slope(&bundle.obv, 10),
                    slope(&bundle.cvd, 10),
                    volumes.get(last).copied().unwrap_or(0.0),
                    get_val(&bundle.sma_20, last),
                    false,
                ),
                qtss_tbm::structure::score_structure(
                    fib_proximity, "nearest",
                    get_val(&bundle.bollinger.percent_b, last),
                    qtss_indicators::volatility::bb_squeeze(&bundle.bollinger.bandwidth, 0.03).get(last).copied().unwrap_or(false),
                    qtss_indicators::volatility::compression_detector(&bundle.atr_14, 10).get(last).copied().unwrap_or(false),
                    form_quality, form_name,
                    false,
                ),
                qtss_tbm::onchain::score_onchain(&onchain_metrics, false),
            ];
            let top_score = score_tbm(top_pillars);

            let setups = detect_setups(&bottom_score, &top_score, &SetupThresholds::default());

            // Telegram bildirimi
            if !setups.is_empty() {
                if let Some((ref d, ref chans)) = tbm_bundle {
                    for s in &setups {
                        notify_tbm_setup(d, chans, &t, s).await;
                    }
                }
            }

            // ── TBM → Execution Bridge ──
            // Strong+ setup → range_signal_event insert → paper/live execution pipeline
            if tbm_auto_execute {
                for s in &setups {
                    let sig_order = match s.signal {
                        TbmSignal::VeryStrong => 5,
                        TbmSignal::Strong => 4,
                        TbmSignal::Moderate => 3,
                        _ => 0,
                    };
                    if sig_order < tbm_execute_min_signal {
                        continue;
                    }
                    let (event_kind, dir_label) = match s.direction {
                        qtss_tbm::setup::SetupDirection::Bottom => ("long_entry", "tbm_bottom"),
                        qtss_tbm::setup::SetupDirection::Top => ("short_entry", "tbm_top"),
                    };
                    let row = RangeSignalEventInsert {
                        engine_symbol_id: t.id,
                        event_kind: event_kind.to_string(),
                        bar_open_time: last_bar_ot,
                        reference_price: Some(last_close),
                        source: "tbm_setup".into(),
                        payload: json!({
                            "direction": dir_label,
                            "score": s.score,
                            "signal": format!("{:?}", s.signal),
                            "pillar_details": s.pillar_details,
                            "summary": s.summary,
                        }),
                    };
                    match insert_range_signal_event(pool, &row).await {
                        Ok(Some(_)) => {
                            info!(
                                symbol = %t.symbol,
                                kind = event_kind,
                                score = format!("{:.1}", s.score),
                                "tbm_execution_event inserted"
                            );
                        }
                        Ok(None) => {} // duplicate, skip
                        Err(e) => warn!(%e, symbol = %t.symbol, "tbm execution event insert"),
                    }
                }
            }

            let tbm_payload = json!({
                "bottom": {
                    "total": bottom_score.total,
                    "signal": format!("{:?}", bottom_score.signal),
                    "pillars": bottom_score.pillars,
                },
                "top": {
                    "total": top_score.total,
                    "signal": format!("{:?}", top_score.signal),
                    "pillars": top_score.pillars,
                },
                "setups": setups,
                "bar_count": bars.len(),
            });

            if let Err(e) = upsert_analysis_snapshot(
                pool, t.id, "tbm_scores", &tbm_payload, last_ot, Some(n as i32), None,
            ).await {
                warn!(%e, symbol = %t.symbol, "tbm_scores snapshot");
            } else {
                info!(
                    symbol = %t.symbol,
                    bottom = format!("{:.1}", bottom_score.total),
                    top = format!("{:.1}", top_score.total),
                    setups = setups.len(),
                    "tbm_scores snapshot"
                );
            }

            // ── MTF konfirmasyon ──
            if let Ok(siblings) = fetch_sibling_tbm_snapshots(pool, &t.exchange, &t.segment, &t.symbol).await {
                let tf_scores: Vec<TfScore> = siblings
                    .iter()
                    .filter_map(|(interval, payload)| {
                        let tf = Timeframe::from_interval(interval)?;
                        let bs = payload.pointer("/bottom/total")?.as_f64()?;
                        let ts = payload.pointer("/top/total")?.as_f64()?;
                        let bsig_str = payload.pointer("/bottom/signal")?.as_str().unwrap_or("None");
                        let tsig_str = payload.pointer("/top/signal")?.as_str().unwrap_or("None");
                        let parse_sig = |s: &str| match s {
                            "VeryStrong" => TbmSignal::VeryStrong,
                            "Strong" => TbmSignal::Strong,
                            "Moderate" => TbmSignal::Moderate,
                            "Weak" => TbmSignal::Weak,
                            _ => TbmSignal::None,
                        };
                        Some(TfScore {
                            timeframe: tf,
                            bottom_score: bs,
                            top_score: ts,
                            bottom_signal: parse_sig(bsig_str),
                            top_signal: parse_sig(tsig_str),
                        })
                    })
                    .collect();

                if tf_scores.len() >= 2 {
                    let mtf = mtf_confirm(&tf_scores);
                    let mtf_payload = json!({
                        "bottom_score": mtf.bottom_score,
                        "top_score": mtf.top_score,
                        "bottom_signal": format!("{:?}", mtf.bottom_signal),
                        "top_signal": format!("{:?}", mtf.top_signal),
                        "bottom_alignment": mtf.bottom_alignment,
                        "top_alignment": mtf.top_alignment,
                        "tf_count": mtf.tf_count,
                        "has_conflict": mtf.has_conflict,
                        "details": mtf.details,
                        "tf_scores": mtf.tf_scores,
                    });
                    if let Err(e) = upsert_analysis_snapshot(
                        pool, t.id, "tbm_mtf", &mtf_payload, last_ot, Some(n as i32), None,
                    ).await {
                        warn!(%e, symbol = %t.symbol, "tbm_mtf snapshot");
                    } else {
                        info!(
                            symbol = %t.symbol,
                            tf_count = mtf.tf_count,
                            bottom = format!("{:.1}", mtf.bottom_score),
                            top = format!("{:.1}", mtf.top_score),
                            alignment = format!("B{}/T{}", mtf.bottom_alignment, mtf.top_alignment),
                            conflict = mtf.has_conflict,
                            "tbm_mtf snapshot"
                        );
                    }

                    // MTF setup sinyali Telegram'a gönder (Strong+ ve alignment ≥2)
                    if let Some((ref d, ref chans)) = tbm_bundle {
                        if (mtf.bottom_score >= 70.0 && mtf.bottom_alignment >= 2)
                            || (mtf.top_score >= 70.0 && mtf.top_alignment >= 2)
                        {
                            let dir = if mtf.bottom_score >= mtf.top_score { "BOTTOM" } else { "TOP" };
                            let best = mtf.bottom_score.max(mtf.top_score);
                            let align = if dir == "BOTTOM" { mtf.bottom_alignment } else { mtf.top_alignment };
                            let emoji = if dir == "BOTTOM" { "🟢" } else { "🔴" };
                            let title = format!(
                                "{emoji} MTF {dir} — {} {} ({}/{} TFs aligned)",
                                t.symbol, t.interval, align, mtf.tf_count
                            );
                            let detail_lines = mtf.details.iter().take(6).map(|d| format!("• {d}")).collect::<Vec<_>>().join("\n");
                            let tf_lines = mtf.tf_scores.iter().map(|s| {
                                format!("  {:?}: B={:.0} T={:.0}", s.timeframe, s.bottom_score, s.top_score)
                            }).collect::<Vec<_>>().join("\n");
                            let body = format!(
                                "MTF Score: {best:.1} | Signal: {:?}\n\n{tf_lines}\n\n{detail_lines}",
                                if dir == "BOTTOM" { mtf.bottom_signal } else { mtf.top_signal },
                            );
                            let notif = Notification::new(title, body);
                            for r in d.send_all(chans, &notif).await {
                                if r.ok {
                                    info!(channel = ?r.channel, symbol = %t.symbol, "MTF TBM bildirimi");
                                } else {
                                    warn!(channel = ?r.channel, detail = ?r.detail, "MTF TBM bildirimi başarısız");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub async fn engine_analysis_loop(pool: PgPool, confluence: Arc<dyn ConfluencePersist>) {
    let bar_limit: i64 = std::env::var("QTSS_ENGINE_BARS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_500)
        .clamp(120, 50_000);

    loop {
        let range_doc = match fetch_range_engine_json(&pool).await {
            Ok(d) => d,
            Err(e) => {
                warn!(
                    %e,
                    qtss_module = "qtss_worker::range_engine",
                    "range_engine app_config okunamadı; varsayılan iskelet"
                );
                default_range_engine_json()
            }
        };
        let params = trading_range_params_from_doc_and_env(&range_doc);
        let refresh_requested = range_doc
            .pointer("/worker/refresh_requested")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if refresh_requested {
            log_business(
                QtssLogLevel::Info,
                "qtss_worker::range_engine",
                "worker.refresh_requested: motor turu (web/app_config)",
            );
        }
        tracing::debug!(
            target: "qtss",
            qtss_module = "qtss_worker::range_engine",
            lookback = params.lookback,
            atr_period = params.atr_period,
            atr_sma_period = params.atr_sma_period,
            require_range_regime = params.require_range_regime,
            refresh_requested,
            "trading_range_params etkin (app_config + env yedek)"
        );

        run_engines_for_symbol(
            &pool,
            bar_limit,
            &params,
            &confluence,
            &range_doc,
        )
        .await;

        if refresh_requested {
            match clear_refresh_requested(&pool).await {
                Ok(()) => {
                    log_business(
                        QtssLogLevel::Info,
                        "qtss_worker::range_engine",
                        "worker.refresh_requested temizlendi",
                    );
                }
                Err(e) => warn!(
                    %e,
                    qtss_module = "qtss_worker::range_engine",
                    "refresh_requested sıfırlanamadı"
                ),
            }
        }

        let sleep_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "engine_analysis_tick_secs",
            "QTSS_ENGINE_TICK_SECS",
            120,
            15,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}
