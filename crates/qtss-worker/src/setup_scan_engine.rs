//! Nansen token screener verisi + dahili skor / seviye kuralları → `nansen_setup_*` tabloları.
//!
//! Hyperliquid: Nansen satırında `hl_long_pct` / `hl_short_pct` yoksa, tarama başına tek istekle
//! `POST https://api.hyperliquid.xyz/info` (`metaAndAssetCtxs`) funding ile skor ve sinyal zenginleştirilir.
//! OHLC Binance `market_bars` (`SYMBOLUSDT`) ile 1h + 6h; TP1 mümkünse 1h high/low ceplerine pinlenir.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use qtss_common::log_critical;
use qtss_nansen::post_token_screener;
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    fetch_nansen_snapshot, insert_nansen_setup_run, insert_nansen_setup_row, list_recent_bars,
    NansenSetupRowInsert, NansenSetupRunInsert,
};
use reqwest::Client;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::nansen_query::{nansen_api_base, token_screener_body};

const SNAPSHOT_KIND: &str = "token_screener";
const SPEC_VERSION: &str = "nansen_setup_v4";
/// Hyperliquid `info` — tek istekte tüm perp coin bağlamı.
const HL_INFO_URL: &str = "https://api.hyperliquid.xyz/info";
/// |funding| bu eşiğin altındayken HL funding skoru/sinyali uygulanmaz.
const HL_FUNDING_MIN_ABS: f64 = 0.00003;
/// Bu mutlak funding ≈ tam skor tavanı (`HL_FUNDING_SCORE_CAP`).
const HL_FUNDING_REF_ABS: f64 = 0.0003;
/// Nansen HL L/S% ile aynı maksimum katkı.
const HL_FUNDING_SCORE_CAP: f64 = 6.0;

/// Varsayılan: **açık** — setup taraması Nansen’e ikinci bir HTTP isteği yapmaz; yalnızca
/// `nansen_snapshots` okur (kredi tek yerde: `nansen_engine`). Canlı yedek: `QTSS_SETUP_SNAPSHOT_ONLY=0`.
fn setup_snapshot_only_mode() -> bool {
    match std::env::var("QTSS_SETUP_SNAPSHOT_ONLY")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        _ => true,
    }
}

fn notify_setup_env_enabled() -> bool {
    std::env::var("QTSS_NOTIFY_SETUP_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn setup_notify_channels_from_env() -> Vec<NotificationChannel> {
    let raw = std::env::var("QTSS_NOTIFY_SETUP_CHANNELS").unwrap_or_else(|_| "telegram".into());
    raw.split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn setup_notify_min_score() -> f64 {
    std::env::var("QTSS_NOTIFY_SETUP_MIN_SCORE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8.0)
}

/// 0–100 (iç skor `probability()` 0–1; karşılaştırma `prob * 100.0` ile).
fn setup_notify_min_probability_pct() -> f64 {
    std::env::var("QTSS_NOTIFY_SETUP_MIN_PROBABILITY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(45.0)
}

fn setup_notify_max_items() -> usize {
    std::env::var("QTSS_NOTIFY_SETUP_MAX_ITEMS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .clamp(1, 20)
}

/// PLAN Phase D — eşik üstü adaylar için tek özet bildirimi (gövde Türkçe; `run_id` başlıkta).
async fn maybe_notify_nansen_setup_run(run_id: Uuid, ranked: &[(f64, Candidate, &'static str, i32)]) {
    if !notify_setup_env_enabled() || ranked.is_empty() {
        return;
    }
    let chans = setup_notify_channels_from_env();
    if chans.is_empty() {
        warn!("QTSS_NOTIFY_SETUP_ENABLED açık fakat QTSS_NOTIFY_SETUP_CHANNELS boş veya geçersiz");
        return;
    }
    let min_sc = setup_notify_min_score();
    let min_pr_pct = setup_notify_min_probability_pct();
    let max_items = setup_notify_max_items();
    let mut lines: Vec<String> = Vec::new();
    for (_k, c, dir, sc) in ranked {
        let entry = c.price_usd;
        let (_sl, _tp1, _tp2, _tp3, rr, _pct, _) = levels(dir, entry, c);
        let prob = probability(*sc, rr);
        let prob_pct = prob * 100.0;
        if (*sc as f64) < min_sc || prob_pct < min_pr_pct {
            continue;
        }
        lines.push(format!(
            "{}. {} {} | skor {:.1} olasılık {:.0}% | {}",
            lines.len() + 1,
            c.token_symbol,
            dir,
            sc,
            prob_pct,
            c.chain
        ));
        if lines.len() >= max_items {
            break;
        }
    }
    if lines.is_empty() {
        return;
    }
    let title = format!("Nansen setup — run {run_id}");
    let body = format!(
        "Eşik: skor ≥ {min_sc:.1}, olasılık ≥ {min_pr_pct:.0}%.\n{}",
        lines.join("\n")
    );
    let d = NotificationDispatcher::from_env();
    let n = Notification::new(title, body);
    for r in d.send_all(&chans, &n).await {
        if r.ok {
            info!(channel = ?r.channel, "nansen_setup bildirimi");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, "nansen_setup bildirimi başarısız");
        }
    }
}

const MIN_LIQUIDITY_USD: f64 = 8_000.0;
const MIN_VOLUME_USD: f64 = 50.0;
const MAX_ABS_PRICE_CHANGE: f64 = 8.0;
const MAX_TRADERS_CROWDED: i64 = 80;
/// Skor sonrası yönü LONG olanlardan en iyi N (olasılık×RR).
const TOP_LONG: usize = 5;
/// Skor sonrası yönü SHORT olanlardan en iyi N.
const TOP_SHORT: usize = 5;
const MAIN_MOVE_PCT: f64 = 0.20;
const SL_PCT_LONG: f64 = 0.085;
const SL_PCT_SHORT: f64 = 0.085;
const TP1_FRAC_OF_MOVE: f64 = 0.42;
const TP3_EXTEND_LONG: f64 = 1.42;
const TP3_EXTEND_SHORT: f64 = 0.58;

fn v_f64(obj: &Value, key: &str) -> Option<f64> {
    obj.get(key).and_then(|x| {
        x.as_f64()
            .or_else(|| x.as_i64().map(|i| i as f64))
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

fn v_i64(obj: &Value, key: &str) -> Option<i64> {
    obj.get(key).and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_f64().map(|f| f as i64))
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

fn v_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn binance_usdt_guess(symbol: &str) -> Option<String> {
    let clean: String = symbol
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    if (2..=20).contains(&clean.len()) {
        Some(format!("{clean}USDT"))
    } else {
        None
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    chain: String,
    token_address: String,
    token_symbol: String,
    price_usd: f64,
    buy_volume: f64,
    sell_volume: f64,
    netflow: f64,
    volume: f64,
    price_change: f64,
    liquidity: f64,
    token_age_days: f64,
    nof_traders: i64,
    raw: Value,
    long_score: f64,
    short_score: f64,
    ohlc_enriched: bool,
    vol_expansion: bool,
    structure_bull: bool,
    structure_bear: bool,
    /// LONG: en yakın 1h direnç (entry ile TP2 arasındaki minimum high).
    tp1_resistance_proxy: Option<f64>,
    /// SHORT: entry ile TP2 arasındaki maksimum low (aşağı ilk likidite cebi).
    tp1_support_proxy: Option<f64>,
    h6_vol_expansion: bool,
}

fn score_candidate(row: &Value) -> Option<Candidate> {
    let chain = v_str(row, "chain")?.to_string();
    let token_address = v_str(row, "token_address")?.to_string();
    let token_symbol = v_str(row, "token_symbol").unwrap_or("?").to_string();
    let price_usd = v_f64(row, "price_usd").filter(|p| *p > 0.0)?;
    let buy_volume = v_f64(row, "buy_volume").unwrap_or(0.0).max(0.0);
    let sell_volume = v_f64(row, "sell_volume").unwrap_or(0.0).max(0.0);
    let netflow = v_f64(row, "netflow").unwrap_or(0.0);
    let volume = v_f64(row, "volume").unwrap_or(buy_volume + sell_volume).max(0.0);
    let price_change = v_f64(row, "price_change").unwrap_or(0.0);
    let liquidity = v_f64(row, "liquidity").unwrap_or(0.0);
    let token_age_days = v_f64(row, "token_age_days").unwrap_or(0.0);
    let nof_traders = v_i64(row, "nof_traders").unwrap_or(0);

    if liquidity < MIN_LIQUIDITY_USD || volume < MIN_VOLUME_USD {
        return None;
    }
    if price_change.abs() > MAX_ABS_PRICE_CHANGE {
        return None;
    }
    if nof_traders > MAX_TRADERS_CROWDED {
        return None;
    }

    let bv = buy_volume + sell_volume + 1e-9;
    let buy_ratio = buy_volume / bv;

    // LONG bileşenleri (0–100 ölçeğine yakın ham puan)
    let mut long_score = 0.0;
    if netflow > 0.0 {
        long_score += ((netflow / volume.max(1.0)).clamp(0.0, 1.0)) * 28.0;
    }
    long_score += (buy_ratio - 0.5).max(0.0) * 2.0 * 22.0;
    if (2.0..=180.0).contains(&token_age_days) {
        long_score += 14.0;
    } else if token_age_days < 2.0 {
        long_score += 10.0;
    }
    long_score += (liquidity / 200_000.0).clamp(0.0, 1.0) * 12.0;
    if (-0.25..=0.55).contains(&price_change) {
        long_score += 12.0;
    }
    if buy_ratio > 0.58 && netflow > 0.0 {
        long_score += 8.0;
    }
    let inflow_fdv = v_f64(row, "inflow_fdv_ratio").unwrap_or(0.0).max(0.0);
    long_score += (inflow_fdv * 100.0).clamp(0.0, 12.0);

    let mut short_score = 0.0;
    if netflow < 0.0 {
        short_score += ((-netflow / volume.max(1.0)).clamp(0.0, 1.0)) * 28.0;
    }
    short_score += (0.5 - buy_ratio).max(0.0) * 2.0 * 22.0;
    if price_change > 0.25 && netflow < 0.0 {
        short_score += 18.0;
    }
    if price_change < -0.08 && buy_ratio < 0.45 {
        short_score += 14.0;
    }
    if token_age_days > 60.0 && price_change < 0.0 {
        short_score += 8.0;
    }
    short_score += (liquidity / 200_000.0).clamp(0.0, 1.0) * 8.0;
    let outflow_fdv = v_f64(row, "outflow_fdv_ratio").unwrap_or(0.0).max(0.0);
    short_score += (outflow_fdv * 100.0).clamp(0.0, 12.0);

    // Screener satırında varsa HL tarafı ağırlık (alan adları API sürümüne göre değişebilir).
    if let (Some(lp), Some(sp)) = (
        v_f64(row, "hyperliquid_long_pct").or_else(|| v_f64(row, "hl_long_pct")),
        v_f64(row, "hyperliquid_short_pct").or_else(|| v_f64(row, "hl_short_pct")),
    ) {
        if lp + sp > 1.0 {
            if lp > sp + 5.0 {
                long_score += 6.0;
            } else if sp > lp + 5.0 {
                short_score += 6.0;
            }
        }
    }

    if let (Some(nb), Some(ns)) = (v_i64(row, "nof_buys"), v_i64(row, "nof_sells")) {
        if nb >= 3 && nb > ns {
            long_score += 6.0;
        }
        if ns >= 3 && ns > nb {
            short_score += 6.0;
        }
    }

    Some(Candidate {
        chain,
        token_address,
        token_symbol,
        price_usd,
        buy_volume,
        sell_volume,
        netflow,
        volume,
        price_change,
        liquidity,
        token_age_days,
        nof_traders,
        raw: row.clone(),
        long_score,
        short_score,
        ohlc_enriched: false,
        vol_expansion: false,
        structure_bull: false,
        structure_bear: false,
        tp1_resistance_proxy: None,
        tp1_support_proxy: None,
        h6_vol_expansion: false,
    })
}

async fn enrich_ohlc(pool: &PgPool, c: &mut Candidate) {
    let Some(pair) = binance_usdt_guess(&c.token_symbol) else {
        return;
    };
    let (r1, r6) = tokio::join!(
        list_recent_bars(pool, "binance", "spot", &pair, "1h", 48),
        list_recent_bars(pool, "binance", "spot", &pair, "6h", 32),
    );
    let Ok(rows) = r1 else {
        return;
    };
    if rows.len() < 18 {
        return;
    }
    let chrono: Vec<_> = rows.into_iter().rev().collect();
    let n = chrono.len();
    let highs: Vec<f64> = chrono
        .iter()
        .filter_map(|r| r.high.to_f64())
        .collect();
    let lows: Vec<f64> = chrono.iter().filter_map(|r| r.low.to_f64()).collect();
    let closes: Vec<f64> = chrono.iter().filter_map(|r| r.close.to_f64()).collect();
    if highs.len() != n || lows.len() != n {
        return;
    }

    let entry = c.price_usd;
    let tp2_long = entry * (1.0 + MAIN_MOVE_PCT);
    let nearest_res = highs
        .iter()
        .copied()
        .filter(|&h| h > entry * 1.0005 && h < tp2_long * 0.9995)
        .fold(f64::INFINITY, f64::min);
    if nearest_res.is_finite() {
        c.tp1_resistance_proxy = Some(nearest_res);
    }
    let tp2_short = entry * (1.0 - MAIN_MOVE_PCT);
    let nearest_sup = lows
        .iter()
        .copied()
        .filter(|&l| l < entry * 0.9995 && l > tp2_short * 1.0005)
        .fold(f64::NEG_INFINITY, f64::max);
    if nearest_sup > f64::NEG_INFINITY {
        c.tp1_support_proxy = Some(nearest_sup);
    }

    let last6_h = highs[n - 6..n].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let last6_l = lows[n - 6..n].iter().cloned().fold(f64::INFINITY, f64::min);
    let prev6_h = highs[n - 12..n - 6]
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let prev6_l = lows[n - 12..n - 6]
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let r_last = (last6_h - last6_l).max(1e-12);
    let r_prev = (prev6_h - prev6_l).max(1e-12);
    c.vol_expansion = r_last > r_prev * 1.12;
    let mid = (last6_h + last6_l) / 2.0;
    let last_c = *closes.last().unwrap_or(&c.price_usd);
    c.structure_bull = last_c >= mid + r_last * 0.08;
    c.structure_bear = last_c <= mid - r_last * 0.08;
    c.ohlc_enriched = true;

    if c.vol_expansion {
        c.long_score += 6.0;
        c.short_score += 6.0;
    }
    if c.structure_bull {
        c.long_score += 8.0;
        c.short_score -= 4.0;
    }
    if c.structure_bear {
        c.short_score += 8.0;
        c.long_score -= 4.0;
    }

    if let Ok(rows6) = r6 {
        if rows6.len() >= 8 {
            let ch6: Vec<_> = rows6.into_iter().rev().collect();
            let nh = ch6.len();
            let highs6: Vec<f64> = ch6.iter().filter_map(|r| r.high.to_f64()).collect();
            let lows6: Vec<f64> = ch6.iter().filter_map(|r| r.low.to_f64()).collect();
            if highs6.len() == nh && lows6.len() == nh {
                let tr: Vec<f64> = (0..nh)
                    .map(|i| (highs6[i] - lows6[i]).max(1e-12))
                    .collect();
                let last = tr[nh - 1];
                let prev_mean: f64 = tr[nh - 5..nh - 1].iter().sum::<f64>() / 4.0;
                c.h6_vol_expansion = last > prev_mean * 1.15;
                if c.h6_vol_expansion {
                    c.long_score += 3.0;
                    c.short_score += 3.0;
                }
            }
        }
    }
}

fn hyperliquid_enrich_enabled() -> bool {
    match std::env::var("QTSS_HYPERLIQUID_ENRICH")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        _ => true,
    }
}

fn nansen_row_has_hl_long_short_pct(row: &Value) -> bool {
    if let (Some(lp), Some(sp)) = (
        v_f64(row, "hyperliquid_long_pct").or_else(|| v_f64(row, "hl_long_pct")),
        v_f64(row, "hyperliquid_short_pct").or_else(|| v_f64(row, "hl_short_pct")),
    ) {
        return lp + sp > 1.0;
    }
    false
}

fn hl_listing_key(symbol: &str) -> String {
    symbol
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase()
}

fn parse_hyperliquid_meta_asset_ctxs(resp: &Value) -> Option<HashMap<String, Value>> {
    let arr = resp.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    let uni = arr[0].get("universe")?.as_array()?;
    let ctxs = arr[1].as_array()?;
    let n = uni.len().min(ctxs.len());
    let mut m = HashMap::with_capacity(n);
    for i in 0..n {
        let name = uni[i].get("name")?.as_str()?.to_uppercase();
        m.insert(name, ctxs[i].clone());
    }
    Some(m)
}

async fn fetch_hyperliquid_asset_ctx_by_coin(client: &Client) -> Option<HashMap<String, Value>> {
    let res = client
        .post(HL_INFO_URL)
        .header("Content-Type", "application/json")
        .json(&json!({ "type": "metaAndAssetCtxs" }))
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let v: Value = res.json().await.ok()?;
    parse_hyperliquid_meta_asset_ctxs(&v)
}

/// Nansen’de HL yüzdesi yoksa, coin adı HL perp listesiyle eşleşen adaylara funding proxy’si uygular.
async fn enrich_hyperliquid(client: &Client, candidates: &mut [Candidate]) {
    if !hyperliquid_enrich_enabled() {
        return;
    }
    let Some(map) = fetch_hyperliquid_asset_ctx_by_coin(client).await else {
        warn!("hyperliquid: metaAndAssetCtxs alınamadı — setup HL API atlandı");
        return;
    };
    for c in candidates.iter_mut() {
        if nansen_row_has_hl_long_short_pct(&c.raw) {
            continue;
        }
        let key = hl_listing_key(&c.token_symbol);
        if key.is_empty() {
            continue;
        }
        let Some(ctx) = map.get(&key) else {
            continue;
        };
        let Some(f) = v_f64(ctx, "funding") else {
            continue;
        };
        let af = f.abs();
        if af < HL_FUNDING_MIN_ABS {
            continue;
        }
        let scale = (af / HL_FUNDING_REF_ABS).clamp(0.0, 1.0);
        let bump = scale * HL_FUNDING_SCORE_CAP;
        if f < 0.0 {
            c.long_score += bump;
        } else {
            c.short_score += bump;
        }
        if let Value::Object(ref mut m) = c.raw {
            m.insert("hl_api_funding".into(), json!(f));
            if let Some(mp) = v_f64(ctx, "markPx") {
                m.insert("hl_api_mark_px".into(), json!(mp));
            }
        }
    }
}

fn direction_and_score(c: &Candidate) -> (&'static str, i32) {
    let long = c.long_score;
    let short = c.short_score;
    if long >= short + 12.0 {
        ("LONG", (long.clamp(0.0, 100.0)) as i32)
    } else if short >= long + 12.0 {
        ("SHORT", (short.clamp(0.0, 100.0)) as i32)
    } else if long >= short {
        ("LONG", (long.clamp(0.0, 100.0)) as i32)
    } else {
        ("SHORT", (short.clamp(0.0, 100.0)) as i32)
    }
}

/// TP1: mümkünse 1h OHLC “likidite cebi” vekili; değilse eski oranlı ara hedef.
/// Dönüşün son `bool`: TP1 yapı vekilinden türetildi mi (clamp sonrası da aynı bayrak).
fn levels(direction: &str, entry: f64, c: &Candidate) -> (f64, f64, f64, f64, f64, f64, bool) {
    // TP2’ye giden mesafenin en az bu kadarı TP1’de katedilsin (çok sıkı TP1 önlemi).
    const TP1_MIN_FRAC_OF_MOVE: f64 = 0.12;
    if direction == "LONG" {
        let sl = entry * (1.0 - SL_PCT_LONG);
        let tp2 = entry * (1.0 + MAIN_MOVE_PCT);
        let move_ = (tp2 - entry).max(1e-12);
        let tp1_frac = entry + move_ * TP1_FRAC_OF_MOVE;
        let floor = entry + move_ * TP1_MIN_FRAC_OF_MOVE;
        let raw = c
            .tp1_resistance_proxy
            .filter(|&x| x > entry && x < tp2 && x >= floor);
        let tp1_used = raw.is_some();
        let tp1 = raw
            .unwrap_or(tp1_frac)
            .clamp(floor, tp2 * 0.999);
        let tp3 = entry * TP3_EXTEND_LONG;
        let rr = (tp2 - entry) / (entry - sl).max(1e-12);
        let pct = (tp2 - entry) / entry * 100.0;
        (sl, tp1, tp2, tp3, rr, pct, tp1_used)
    } else {
        let sl = entry * (1.0 + SL_PCT_SHORT);
        let tp2 = entry * (1.0 - MAIN_MOVE_PCT);
        let move_ = (entry - tp2).max(1e-12);
        let tp1_frac = entry - move_ * TP1_FRAC_OF_MOVE;
        let ceil = entry - move_ * TP1_MIN_FRAC_OF_MOVE;
        let floor = tp2 + move_ * 0.02;
        let raw = c
            .tp1_support_proxy
            .filter(|&x| x > tp2 && x < entry && x <= ceil && x >= floor);
        let tp1_used = raw.is_some();
        let tp1 = raw.unwrap_or(tp1_frac).clamp(floor, ceil);
        let tp3 = entry * TP3_EXTEND_SHORT;
        let rr = (entry - tp2) / (sl - entry).max(1e-12);
        let pct = (entry - tp2) / entry * 100.0;
        (sl, tp1, tp2, tp3, rr, pct, tp1_used)
    }
}

fn probability(score: i32, rr: f64) -> f64 {
    let s = (score as f64 / 100.0).clamp(0.0, 1.0);
    let r = (rr / 4.0).clamp(0.0, 1.0);
    (s * 0.65 + r * 0.35).clamp(0.05, 0.99)
}

fn rank_key(prob: f64, rr: f64) -> f64 {
    prob * rr.min(6.0)
}

/// Nansen cevabında alan varsa `key_signals` içine ek açıklama (skorla çakışanlar hariç tutuldu).
fn append_extra_screener_signals(raw: &Value, direction: &str, out: &mut Vec<String>) {
    const EPS: f64 = 1e-9;
    if let (Some(lp), Some(sp)) = (
        v_f64(raw, "hyperliquid_long_pct").or_else(|| v_f64(raw, "hl_long_pct")),
        v_f64(raw, "hyperliquid_short_pct").or_else(|| v_f64(raw, "hl_short_pct")),
    ) {
        if lp + sp > 1.0 {
            if direction == "LONG" && lp > sp + 3.0 {
                out.push(format!(
                    "Screener HL bias: long {:.1}% vs short {:.1}%",
                    lp, sp
                ));
            } else if direction == "SHORT" && sp > lp + 3.0 {
                out.push(format!(
                    "Screener HL bias: short {:.1}% vs long {:.1}%",
                    sp, lp
                ));
            }
        }
    }
    let has_screener_hl = out.iter().any(|s| s.contains("Screener HL bias"));
    if !has_screener_hl {
        if let Some(f) = v_f64(raw, "hl_api_funding") {
            if f.abs() > HL_FUNDING_MIN_ABS {
                if direction == "LONG" && f < 0.0 {
                    out.push(format!(
                        "Hyperliquid API funding {:.6} (short-crowding proxy)",
                        f
                    ));
                } else if direction == "SHORT" && f > 0.0 {
                    out.push(format!(
                        "Hyperliquid API funding {:.6} (long-crowding proxy)",
                        f
                    ));
                }
            }
        }
    }
    for key in ["cex_net_flow", "cex_netflow", "exchange_netflow", "net_cex_flow"] {
        if let Some(x) = v_f64(raw, key) {
            if x.abs() > EPS {
                out.push(format!("Screener `{key}` = {x:.6}"));
            }
            break;
        }
    }
    if let Some(w) = v_f64(raw, "whale_deposits_usd") {
        if w > 1_000.0 {
            out.push(format!("Whale deposits (screener) ≈ ${w:.0}"));
        }
    }
    if let Some(w) = v_f64(raw, "whale_withdrawals_usd") {
        if w > 1_000.0 {
            out.push(format!("Whale withdrawals (screener) ≈ ${w:.0}"));
        }
    }
}

fn build_signals(c: &Candidate, direction: &str) -> Vec<String> {
    let mut v = Vec::new();
    if c.netflow > 0.0 {
        v.push("Smart-money net inflow (proxy)".into());
    } else if c.netflow < 0.0 {
        v.push("Smart-money net outflow (proxy)".into());
    }
    if c.buy_volume > c.sell_volume * 1.15 {
        v.push("DEX buy pressure vs sell".into());
    } else if c.sell_volume > c.buy_volume * 1.15 {
        v.push("DEX sell pressure vs buy".into());
    }
    if let Some(r) = v_f64(&c.raw, "inflow_fdv_ratio").filter(|x| *x > 0.0005) {
        v.push(format!("Inflow/FDV ratio {:.5} (accumulation / CEX flow proxy)", r));
    }
    if let Some(r) = v_f64(&c.raw, "outflow_fdv_ratio").filter(|x| *x > 0.0005) {
        v.push(format!("Outflow/FDV ratio {:.5} (distribution / outflow proxy)", r));
    }
    if let (Some(nb), Some(ns)) = (v_i64(&c.raw, "nof_buys"), v_i64(&c.raw, "nof_sells")) {
        if nb > ns {
            v.push(format!("Multi-wallet buy tx bias: {nb} buys vs {ns} sells (screener)"));
        } else if ns > nb {
            v.push(format!("Sell tx bias: {ns} sells vs {nb} buys (screener)"));
        }
    }
    if (2.0..=120.0).contains(&c.token_age_days) {
        v.push("Token age in early/mid band".into());
    }
    if c.volume >= 50_000.0 {
        v.push(format!("Screener notional volume ≈ ${:.0}", c.volume));
    }
    if c.nof_traders <= 5 {
        v.push("Few tagged smart-money traders (low crowding proxy)".into());
    } else if c.nof_traders >= 30 {
        v.push("Many tagged traders — watch crowding".into());
    }
    if c.vol_expansion {
        v.push("1h volatility expansion vs prior 6×1h window".into());
    }
    if c.h6_vol_expansion {
        v.push("6h bar range expansion vs prior 6h bars (proxy)".into());
    }
    if c.structure_bull {
        v.push("Price structure leaning up (1h)".into());
    }
    if c.structure_bear {
        v.push("Price structure leaning down (1h)".into());
    }
    if !c.ohlc_enriched {
        v.push("No Binance 1h bars for symbol guess — on-chain-only metrics".into());
    }
    if direction == "SHORT" && c.price_change > 0.2 && c.netflow < 0.0 {
        v.push("Post-move distribution proxy (up + net out)".into());
    }
    append_extra_screener_signals(&c.raw, direction, &mut v);
    v
}

fn setup_sentence(direction: &str, c: &Candidate) -> String {
    format!(
        "{direction} — Smart-money 6h screener + flow/FDV + OHLC; TP2 ≈ ±{:.0}%; SL {:.1}% (structure proxy). Liq ~${:.0}.",
        MAIN_MOVE_PCT * 100.0,
        if direction == "LONG" {
            SL_PCT_LONG * 100.0
        } else {
            SL_PCT_SHORT * 100.0
        },
        c.liquidity
    )
}

async fn load_screener_response(
    pool: &PgPool,
    client: &Client,
    base: &str,
    api_key: &str,
    request_body: &Value,
    max_age_secs: i64,
    snapshot_only: bool,
) -> Result<(Value, bool), String> {
    let snap_row = fetch_nansen_snapshot(pool, SNAPSHOT_KIND)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(snap) = snap_row {
        if snap.error.is_none() {
            if let Some(resp) = snap.response_json {
                if snapshot_only {
                    return Ok((resp, true));
                }
                let age = Utc::now()
                    .signed_duration_since(snap.computed_at)
                    .num_seconds();
                if age <= max_age_secs {
                    return Ok((resp, true));
                }
            }
        }
    }

    if snapshot_only {
        return Err(
            "setup: QTSS_SETUP_SNAPSHOT_ONLY=1 — nansen_snapshots yok, hatalı veya boş; \
             Nansen kredisi yalnızca nansen_engine ile tüketilir. Canlı yedek için QTSS_SETUP_SNAPSHOT_ONLY=0."
                .into(),
        );
    }

    let res = post_token_screener(client, base, api_key, request_body).await;
    let (j, _) = match res {
        Ok(pair) => pair,
        Err(e) => {
            if e.is_insufficient_credits() {
                log_critical(
                    "qtss_worker_nansen_setup",
                    "Nansen kredisi tükendi (Insufficient credits). Setup taraması canlı token-screener \
                     çağrısı başarısız; önbellekte geçerli snapshot yoksa veya süresi dolmuşsa skor üretilemez.",
                );
            }
            return Err(e.to_string());
        }
    };
    Ok((j, false))
}

async fn run_one_scan(
    pool: &PgPool,
    client: &Client,
    base: &str,
    api_key: &str,
    request_body: &Value,
) -> Result<(), String> {
    let snapshot_only = setup_snapshot_only_mode();
    let max_age_secs: i64 = std::env::var("QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_200)
        .max(60);

    let (response_json, use_snap) = load_screener_response(
        pool,
        client,
        base,
        api_key,
        request_body,
        max_age_secs,
        snapshot_only,
    )
    .await?;

    let data = response_json
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut candidates: Vec<Candidate> = Vec::new();
    for row in &data {
        if let Some(mut c) = score_candidate(row) {
            enrich_ohlc(pool, &mut c).await;
            candidates.push(c);
        }
    }

    enrich_hyperliquid(client, &mut candidates).await;

    let filtered_n = candidates.len();
    let mut long_ranked: Vec<(f64, Candidate, &'static str, i32)> = Vec::new();
    let mut short_ranked: Vec<(f64, Candidate, &'static str, i32)> = Vec::new();
    for c in candidates {
        let (dir, sc) = direction_and_score(&c);
        let entry = c.price_usd;
        let (_sl, _tp1, _tp2, _tp3, rr, _pct, _) = levels(dir, entry, &c);
        let prob = probability(sc, rr);
        let key = rank_key(prob, rr);
        if dir == "LONG" {
            long_ranked.push((key, c, dir, sc));
        } else {
            short_ranked.push((key, c, dir, sc));
        }
    }
    long_ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    short_ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    long_ranked.truncate(TOP_LONG);
    short_ranked.truncate(TOP_SHORT);
    let ranked: Vec<(f64, Candidate, &'static str, i32)> =
        long_ranked.into_iter().chain(short_ranked.into_iter()).collect();

    let meta = json!({
        "spec_version": SPEC_VERSION,
        "nansen_snapshot_only": snapshot_only,
        "used_cached_snapshot": use_snap,
        "input_row_count": data.len(),
        "candidates_after_filters": filtered_n,
        "top_long": TOP_LONG,
        "top_short": TOP_SHORT,
        "output_rows": ranked.len(),
    });

    let run = NansenSetupRunInsert {
        request_json: request_body.clone(),
        source: "token_screener".into(),
        candidate_count: filtered_n as i32,
        meta_json: Some(meta.clone()),
        error: None,
    };
    let run_id = insert_nansen_setup_run(pool, &run)
        .await
        .map_err(|e| e.to_string())?;

    for (i, (_k, c, dir, sc)) in ranked.iter().enumerate() {
        let entry = c.price_usd;
        let (sl, tp1, tp2, tp3, rr, pct_to_tp2, tp1_struct) = levels(dir, entry, c);
        let prob = probability(*sc, rr);
        let mut signals = build_signals(c, dir);
        if tp1_struct {
            signals.insert(
                0,
                if *dir == "LONG" {
                    "TP1: nearest 1h high between entry and TP2 (liquidity pocket proxy)".into()
                } else {
                    "TP1: nearest 1h low between TP2 and entry (liquidity pocket proxy)".into()
                },
            );
        }
        let setup = setup_sentence(dir, c);
        let row_ins = NansenSetupRowInsert {
            rank: (i + 1) as i32,
            chain: c.chain.clone(),
            token_address: c.token_address.clone(),
            token_symbol: c.token_symbol.clone(),
            direction: dir.to_string(),
            score: *sc,
            probability: prob,
            setup,
            key_signals: json!(signals),
            entry,
            stop_loss: sl,
            tp1,
            tp2,
            tp3,
            rr,
            pct_to_tp2,
            ohlc_enriched: c.ohlc_enriched,
            raw_metrics: c.raw.clone(),
        };
        insert_nansen_setup_row(pool, run_id, &row_ins)
            .await
            .map_err(|e| e.to_string())?;
    }

    maybe_notify_nansen_setup_run(run_id, &ranked).await;

    info!(rows = ranked.len(), %run_id, "nansen_setup_scan tamamlandı");
    Ok(())
}

pub async fn nansen_setup_scan_loop(pool: PgPool) {
    let secs: u64 = std::env::var("QTSS_SETUP_SCAN_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900)
        .max(180);

    let client = match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "setup_scan: reqwest client");
            return;
        }
    };

    let base = nansen_api_base();
    let mut logged_mode = false;

    loop {
        let snapshot_only = setup_snapshot_only_mode();
        if !logged_mode {
            logged_mode = true;
            info!(
                %base,
                %secs,
                spec = SPEC_VERSION,
                %snapshot_only,
                "nansen setup scan — snapshot_only=true iken Nansen HTTP yalnız nansen_engine’da (kredi tasarrufu)"
            );
        }

        let api_key = std::env::var("NANSEN_API_KEY").unwrap_or_default();
        if !snapshot_only && api_key.trim().is_empty() {
            tracing::debug!("setup_scan: canlı Nansen için NANSEN_API_KEY gerekli — atlanıyor");
            tokio::time::sleep(Duration::from_secs(secs)).await;
            continue;
        }

        let body = token_screener_body(&pool).await;
        if let Err(e) = run_one_scan(&pool, &client, &base, api_key.trim(), &body).await {
            warn!(%e, "nansen_setup_scan başarısız");
            let run = NansenSetupRunInsert {
                request_json: body.clone(),
                source: "token_screener".into(),
                candidate_count: 0,
                meta_json: Some(json!({ "spec_version": SPEC_VERSION })),
                error: Some(
                    e.chars()
                        .take(2000)
                        .collect::<String>(),
                ),
            };
            if let Err(e2) = insert_nansen_setup_run(&pool, &run).await {
                warn!(%e2, "nansen_setup_runs hata satırı");
            }
        }

        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}
