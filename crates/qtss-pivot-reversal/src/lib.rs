//! qtss-pivot-reversal — Faz 13
//!
//! Pivot-bazlı **Dip/Tepe (reversal) detector** saf modül.
//!
//! * Asset-class agnostik (CLAUDE.md #4) — sadece `PivotRow`
//!   ve config tablosunu görür.
//! * Mode-agnostik (CLAUDE.md #5) — backtest / live / dry ayrımı
//!   *çağıran* (sweep/live-hook/replay) tarafında yapılır, bu
//!   modül sadece `DetectionDraft` üretir.
//! * If/else minimize (CLAUDE.md #1) — klasifikasyon küçük
//!   look-up tablo; skor per-level config look-up.
//! * Hiçbir sabit yok (CLAUDE.md #2) — tüm skor/floor DB
//!   `config_schema` default'larından okunur.
//!
//! **Üretilen hedefler:**
//!   * **A — R-multiple.** entry = entry_anchor, SL = prev_opp,
//!     TP1 = entry ± tp1_r · R, TP2 = entry ± tp2_r · R.
//!   * **B — Fibonacci.** impulse bacağı = [prev_opp, entry_anchor],
//!     seviyeler = 0.382 / 0.618 / 1.0 / 1.272 / 1.618.
//!   Her ikisi de `raw_meta.targets.{a,b}` altında yazılır, böylece
//!   UI (Radar+Chart) okur, AI feature writer aynı şeyi `training_set`'e
//!   aktarır.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

pub use qtss_domain::v2::pivot::PivotLevel;

// ─── Girdi tipleri (callers fill in) ─────────────────────────────

#[derive(Debug, Clone)]
pub struct PivotRow {
    pub bar_index: i64,
    pub open_time: DateTime<Utc>,
    pub price: Decimal,
    /// "High" | "Low"
    pub kind: String,
    pub prominence: Option<f64>,
    /// "HH" | "HL" | "LH" | "LL" — classify_swing çıktısı.
    pub swing_type: Option<String>,
}

// ─── Tier + StructureEvent ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tier {
    Reactive,
    Major,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Reactive => "reactive",
            Tier::Major => "major",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureEvent {
    ChochBull,
    ChochBear,
    BosBull,
    BosBear,
    Neutral,
}

impl StructureEvent {
    pub fn event_tag(self) -> &'static str {
        match self {
            StructureEvent::ChochBull | StructureEvent::ChochBear => "choch",
            StructureEvent::BosBull | StructureEvent::BosBear => "bos",
            StructureEvent::Neutral => "neutral",
        }
    }
    pub fn direction(self) -> &'static str {
        match self {
            StructureEvent::ChochBull | StructureEvent::BosBull => "bull",
            StructureEvent::ChochBear | StructureEvent::BosBear => "bear",
            StructureEvent::Neutral => "none",
        }
    }
    pub fn is_bearish(self) -> bool {
        matches!(self, StructureEvent::ChochBear | StructureEvent::BosBear)
    }
    pub fn is_choch(self) -> bool {
        matches!(self, StructureEvent::ChochBull | StructureEvent::ChochBear)
    }
}

/// Sliding window classification: (curr_swing, prev_same_swing) → event.
/// Look-up, değil if zinciri.
pub fn classify(curr: Option<&str>, prev_same: Option<&str>) -> StructureEvent {
    match (curr, prev_same) {
        (Some("HH"), Some("LH")) => StructureEvent::ChochBull,
        (Some("LL"), Some("HL")) => StructureEvent::ChochBear,
        (Some("HH"), Some("HH")) | (Some("HL"), Some("HL")) => StructureEvent::BosBull,
        (Some("LL"), Some("LL")) | (Some("LH"), Some("LH")) => StructureEvent::BosBear,
        _ => StructureEvent::Neutral,
    }
}

// ─── Config ──────────────────────────────────────────────────────

/// Per sweep/loop bir kez DB'den yükle, detector invocation'a aktar.
#[derive(Debug, Clone)]
pub struct ReversalConfig {
    pub tier_by_level: HashMap<String, Tier>,
    /// (level_str, event_tag) → tier_score. event_tag ∈ {choch,bos,neutral}.
    pub score: HashMap<(String, &'static str), f32>,
    pub prominence_floor: HashMap<String, f64>,
    // Hedef parametreleri (outcome-eval ile aynı key'ler):
    pub reactive_tp1_r: f64,
    pub reactive_tp2_r: f64,
    pub major_tp1_r: f64,
    pub major_tp2_r: f64,
    pub fib_levels: Vec<f64>,
}

impl Default for ReversalConfig {
    fn default() -> Self {
        let mut tier_by_level = HashMap::new();
        tier_by_level.insert("L0".into(), Tier::Reactive);
        tier_by_level.insert("L1".into(), Tier::Reactive);
        tier_by_level.insert("L2".into(), Tier::Major);
        tier_by_level.insert("L3".into(), Tier::Major);
        let defaults = [
            ("L0", "choch", 0.50_f32), ("L0", "bos", 0.30), ("L0", "neutral", 0.15),
            ("L1", "choch", 0.65), ("L1", "bos", 0.40), ("L1", "neutral", 0.20),
            ("L2", "choch", 0.85), ("L2", "bos", 0.60), ("L2", "neutral", 0.30),
            ("L3", "choch", 0.95), ("L3", "bos", 0.70), ("L3", "neutral", 0.40),
        ];
        let mut score = HashMap::new();
        for (l, e, v) in defaults {
            score.insert((l.to_string(), e), v);
        }
        let mut prominence_floor = HashMap::new();
        for l in ["L0", "L1", "L2", "L3"] {
            prominence_floor.insert(l.to_string(), 0.0);
        }
        Self {
            tier_by_level,
            score,
            prominence_floor,
            reactive_tp1_r: 1.0,
            reactive_tp2_r: 2.0,
            major_tp1_r: 1.5,
            major_tp2_r: 3.0,
            fib_levels: vec![0.382, 0.618, 1.0, 1.272, 1.618],
        }
    }
}

impl ReversalConfig {
    /// `config_schema`'dan tüm Faz 13 key'lerini oku. Eksikler
    /// `Default`'tan gelir.
    pub async fn load(pool: &sqlx::PgPool) -> anyhow::Result<Self> {
        async fn get_json(pool: &sqlx::PgPool, key: &str) -> Option<serde_json::Value> {
            sqlx::query_scalar::<_, Option<serde_json::Value>>(
                "SELECT default_value FROM config_schema WHERE key = $1",
            )
            .bind(key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .flatten()
        }
        async fn get_f(pool: &sqlx::PgPool, key: &str, fb: f64) -> f64 {
            get_json(pool, key).await.and_then(|v| v.as_f64()).unwrap_or(fb)
        }

        let mut cfg = Self::default();

        for (key, tier) in [
            ("pivot_reversal.tier.reactive.levels", Tier::Reactive),
            ("pivot_reversal.tier.major.levels", Tier::Major),
        ] {
            if let Some(arr) = get_json(pool, key).await.and_then(|v| v.as_array().cloned()) {
                for item in arr {
                    if let Some(lvl) = item.as_str() {
                        cfg.tier_by_level.insert(lvl.to_string(), tier);
                    }
                }
            }
        }
        let events = [("choch", 0.0), ("bos", 0.0), ("neutral", 0.0)];
        for lvl in ["L0", "L1", "L2", "L3"] {
            for (ev, _) in events {
                let key = format!("pivot_reversal.score.{}.{}", lvl, ev);
                let current = *cfg.score.get(&(lvl.to_string(), ev)).unwrap_or(&0.3);
                let v = get_f(pool, &key, current as f64).await as f32;
                cfg.score.insert((lvl.to_string(), ev), v);
            }
            let pkey = format!("pivot_reversal.prominence_floor.{}", lvl);
            cfg.prominence_floor.insert(lvl.to_string(), get_f(pool, &pkey, 0.0).await);
        }
        cfg.reactive_tp1_r = get_f(pool, "eval.pivot_reversal.reactive.tp1_r", cfg.reactive_tp1_r).await;
        cfg.reactive_tp2_r = get_f(pool, "eval.pivot_reversal.reactive.tp2_r", cfg.reactive_tp2_r).await;
        cfg.major_tp1_r = get_f(pool, "eval.pivot_reversal.major.tp1_r", cfg.major_tp1_r).await;
        cfg.major_tp2_r = get_f(pool, "eval.pivot_reversal.major.tp2_r", cfg.major_tp2_r).await;
        Ok(cfg)
    }

    pub fn tier(&self, level: PivotLevel) -> Tier {
        self.tier_by_level.get(level.as_str()).copied().unwrap_or(Tier::Reactive)
    }
    pub fn score_of(&self, level: PivotLevel, event: &'static str) -> f32 {
        self.score.get(&(level.as_str().to_string(), event)).copied().unwrap_or(0.30)
    }
    pub fn floor(&self, level: PivotLevel) -> f64 {
        self.prominence_floor.get(level.as_str()).copied().unwrap_or(0.0)
    }
    pub fn tp_pair(&self, tier: Tier) -> (f64, f64) {
        match tier {
            Tier::Reactive => (self.reactive_tp1_r, self.reactive_tp2_r),
            Tier::Major    => (self.major_tp1_r, self.major_tp2_r),
        }
    }
}

// ─── Detection draft (pure output) ───────────────────────────────

/// Hazır-yazılabilir detection satırı. Çağıran (sweep/live-hook)
/// `mode` + UUID + exchange/symbol/tf'yi ekler, DB'ye INSERT eder.
#[derive(Debug, Clone, Serialize)]
pub struct DetectionDraft {
    pub detected_at: DateTime<Utc>,
    pub subkind: String,
    pub tier: Tier,
    pub event: StructureEvent,
    pub bearish: bool,
    pub structural_score: f32,
    pub invalidation_price: Decimal,
    pub pivot_level: PivotLevel,
    pub anchors: Json,
    pub raw_meta: Json,
    /// A hedefleri (operatör + outcome-eval okur).
    pub targets_a: TargetsA,
    /// B hedefleri (Chart overlay + AI feature).
    pub targets_b: TargetsB,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetsA {
    pub entry: f64,
    pub sl: f64,
    pub tp1_r: f64,
    pub tp2_r: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub risk_dist: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetsB {
    pub impulse_lo: f64,
    pub impulse_hi: f64,
    /// seviye → fiyat. Örn: "0.618" → 41913.0
    pub fib: HashMap<String, f64>,
}

// ─── Core: build_detection() ─────────────────────────────────────

/// Sliding window içinden tek pivot için draft üret.
/// Null döner → prominence/neutral-neutral filtreleri ile skip.
pub fn build_detection(
    pivots: &[PivotRow],
    i: usize,
    level: PivotLevel,
    cfg: &ReversalConfig,
) -> Option<DetectionDraft> {
    if i < 2 || i >= pivots.len() {
        return None;
    }
    let curr = &pivots[i];
    if let Some(p) = curr.prominence {
        if p < cfg.floor(level) {
            return None;
        }
    }
    let prev_same = pivots[..i].iter().rev().find(|p| p.kind == curr.kind)?;
    let prev_opp = pivots[..i].iter().rev().find(|p| p.kind != curr.kind)?;

    let ev = classify(curr.swing_type.as_deref(), prev_same.swing_type.as_deref());
    let tier = cfg.tier(level);
    let subkind = format!(
        "{}_{}_{}_{}",
        tier.as_str(),
        ev.event_tag(),
        ev.direction(),
        level.as_str()
    );
    let bearish = ev.is_bearish();

    let entry_anchor = if ev.is_choch() { prev_opp } else { curr };
    // SL = structural invalidation. For BOS the prev_opp is the
    // natural stop (entry=curr, SL=opposite). For CHoCH the
    // entry_anchor *is* prev_opp (the last swing high/low before the
    // reversal). The SL must sit on the OPPOSITE side of entry to
    // invalidate the reversal thesis — above entry for a bearish
    // reversal, below entry for a bullish reversal.
    //
    // Earlier version took nth(1) of same-kind prior pivots, but in
    // an uptrend those are lower highs (not higher), producing an SL
    // below entry for a bearish CHoCH — geometrically impossible for
    // a short. Fix: scan same-kind prior pivots and pick the one on
    // the right side of entry. Fallback to prev_opp-based buffered SL
    // if no such pivot exists.
    let sl_anchor = if ev.is_choch() {
        let candidates: Vec<&_> = pivots[..i]
            .iter()
            .rev()
            .filter(|p| p.kind == entry_anchor.kind)
            .collect();
        let right_side = candidates.iter().find(|p| {
            if bearish {
                p.price > entry_anchor.price
            } else {
                p.price < entry_anchor.price
            }
        });
        right_side.copied().unwrap_or(prev_same)
    } else {
        prev_opp
    };
    let tier_score = cfg.score_of(level, ev.event_tag());
    let prom_part = curr
        .prominence
        .map(|p| (p / 5.0).clamp(0.0, 1.0) as f32)
        .unwrap_or(0.5);
    let score = (tier_score * 0.7 + prom_part * 0.3).min(1.0);

    // Hedefler.
    let entry = entry_anchor.price.to_f64().unwrap_or(0.0);
    let sl = sl_anchor.price.to_f64().unwrap_or(0.0);
    let risk_dist = (entry - sl).abs();
    let (tp1_r, tp2_r) = cfg.tp_pair(tier);
    let (tp1, tp2) = if bearish {
        (entry - tp1_r * risk_dist, entry - tp2_r * risk_dist)
    } else {
        (entry + tp1_r * risk_dist, entry + tp2_r * risk_dist)
    };
    let targets_a = TargetsA { entry, sl, tp1_r, tp2_r, tp1, tp2, risk_dist };

    // Fib: impulse = prev_opp → entry_anchor. Yön bağımsız seviyeler.
    let lo = entry.min(sl);
    let hi = entry.max(sl);
    let range = hi - lo;
    let mut fib: HashMap<String, f64> = HashMap::new();
    for &f in &cfg.fib_levels {
        // Bearish: impulse yukarıdan aşağıya; fiyat lo + (1-f)·range (symmetric extension).
        let price = if bearish {
            lo + (1.0 - f) * range
        } else {
            lo + f * range
        };
        fib.insert(format!("{:.3}", f), price);
    }
    let targets_b = TargetsB { impulse_lo: lo, impulse_hi: hi, fib };

    let anchors = json!([
        {
            "bar_index": prev_opp.bar_index,
            "price":     prev_opp.price.to_string(),
            "level":     level.as_str(),
            "label":     "prev_opp",
            "time":      prev_opp.open_time.to_rfc3339(),
            "swing_type": prev_opp.swing_type,
        },
        {
            "bar_index": entry_anchor.bar_index,
            "price":     entry_anchor.price.to_string(),
            "level":     level.as_str(),
            "label":     if bearish { "top" } else { "bottom" },
            "time":      entry_anchor.open_time.to_rfc3339(),
            "swing_type": entry_anchor.swing_type,
        },
        {
            "bar_index": curr.bar_index,
            "price":     curr.price.to_string(),
            "level":     level.as_str(),
            "label":     "confirm",
            "time":      curr.open_time.to_rfc3339(),
            "swing_type": curr.swing_type,
        }
    ]);

    let raw_meta = json!({
        "faz":        "13",
        "tier":       tier.as_str(),
        "event":      ev.event_tag(),
        "direction":  ev.direction(),
        "bearish":    bearish,
        "prominence": curr.prominence,
        "tier_score": tier_score,
        "swing_type_curr":     curr.swing_type,
        "swing_type_prev_opp": prev_opp.swing_type,
        "targets": {
            "a": {
                "entry":      targets_a.entry,
                "sl":         targets_a.sl,
                "tp1_r":      targets_a.tp1_r,
                "tp2_r":      targets_a.tp2_r,
                "tp1":        targets_a.tp1,
                "tp2":        targets_a.tp2,
                "risk_dist":  targets_a.risk_dist,
            },
            "b": {
                "impulse_lo": targets_b.impulse_lo,
                "impulse_hi": targets_b.impulse_hi,
                "fib":        targets_b.fib,
            }
        }
    });

    Some(DetectionDraft {
        detected_at: curr.open_time,
        subkind,
        tier,
        event: ev,
        bearish,
        structural_score: score,
        invalidation_price: prev_opp.price,
        pivot_level: level,
        anchors,
        raw_meta,
        targets_a,
        targets_b,
    })
}

// ─── AI feature payload ──────────────────────────────────────────

/// Flat feature dict for `qtss_features_snapshot.features_json`
/// (`source='pivot_reversal'`). Designed to be consumed by the
/// LightGBM trainer unchanged — all fields are scalar f64 or
/// categorical string (one-hot friendly). Hedef değerleri R cinsinden
/// tutulur (fiyat-yüzeyi taşımasın), fib seviyeleri ise entry'den
/// sapma yüzdesi olarak normalize edilir.
pub fn features_for(draft: &DetectionDraft) -> serde_json::Value {
    let a = &draft.targets_a;
    let b = &draft.targets_b;
    let entry = a.entry;
    let range_pct = if entry != 0.0 {
        (b.impulse_hi - b.impulse_lo) / entry.abs()
    } else {
        0.0
    };
    // Fib seviyeleri → entry'ye göre % sapma. İsimler sabit: fib_0382 …
    let mut fib_feats = serde_json::Map::new();
    for (k, v) in &b.fib {
        let clean: String = k.chars().filter(|c| c.is_ascii_digit()).collect();
        if entry != 0.0 {
            fib_feats.insert(
                format!("fib_{}_pct", clean),
                serde_json::json!(((v - entry) / entry.abs()) * 100.0),
            );
        }
    }
    serde_json::json!({
        "spec_version":       1,
        "tier":               draft.tier.as_str(),
        "event":              draft.event.event_tag(),
        "direction":          draft.event.direction(),
        "level":              draft.pivot_level.as_str(),
        "is_choch":           draft.event.is_choch() as i32,
        "is_bearish":         draft.bearish as i32,
        "structural_score":   draft.structural_score as f64,
        "risk_dist":          a.risk_dist,
        "risk_pct":           if entry != 0.0 { a.risk_dist / entry.abs() } else { 0.0 },
        "tp1_r":              a.tp1_r,
        "tp2_r":              a.tp2_r,
        "impulse_range_pct":  range_pct,
        "fib":                serde_json::Value::Object(fib_feats),
    })
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_choch_bull() {
        assert_eq!(classify(Some("HH"), Some("LH")), StructureEvent::ChochBull);
    }
    #[test]
    fn classify_choch_bear() {
        assert_eq!(classify(Some("LL"), Some("HL")), StructureEvent::ChochBear);
    }
    #[test]
    fn classify_bos_bull_continuation() {
        assert_eq!(classify(Some("HH"), Some("HH")), StructureEvent::BosBull);
        assert_eq!(classify(Some("HL"), Some("HL")), StructureEvent::BosBull);
    }
    #[test]
    fn classify_bos_bear_continuation() {
        assert_eq!(classify(Some("LL"), Some("LL")), StructureEvent::BosBear);
        assert_eq!(classify(Some("LH"), Some("LH")), StructureEvent::BosBear);
    }
    #[test]
    fn classify_neutral_on_unknown() {
        assert_eq!(classify(None, Some("HH")), StructureEvent::Neutral);
        assert_eq!(classify(Some("HH"), None), StructureEvent::Neutral);
    }
}
