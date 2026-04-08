use crate::channels::{
    ConfirmationChannel, HistoricalHitRate, MultiTimeframeConfluence, RegimeAlignment,
};
use crate::config::ValidatorConfig;
use crate::context::{pattern_key, HitRateStat, ValidationContext};
use crate::engine::Validator;
use chrono::Utc;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal_macros::dec;
use std::sync::Arc;

fn instrument() -> Instrument {
    Instrument {
        venue: Venue::Binance,
        asset_class: AssetClass::CryptoSpot,
        symbol: "BTCUSDT".into(),
        quote_ccy: "USDT".into(),
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        session: SessionCalendar::binance_24x7(),
    }
}

fn regime(kind: RegimeKind, strength: TrendStrength) -> RegimeSnapshot {
    RegimeSnapshot {
        at: Utc::now(),
        kind,
        trend_strength: strength,
        adx: dec!(28),
        bb_width: dec!(0.06),
        atr_pct: dec!(0.02),
        choppiness: dec!(45),
        confidence: 0.8,
    }
}

fn detection(kind: PatternKind, tf: Timeframe, score: f32, regime: RegimeSnapshot) -> Detection {
    Detection::new(
        instrument(),
        tf,
        kind,
        PatternState::Forming,
        vec![],
        score,
        dec!(0),
        regime,
    )
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    ValidatorConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_bad_structural_weight() {
    let mut c = ValidatorConfig::defaults();
    c.structural_weight = 1.5;
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_negative_channel_weight() {
    let mut c = ValidatorConfig::defaults();
    c.channel_weights.push(("bad".into(), -1.0));
    assert!(c.validate().is_err());
}

#[test]
fn config_weight_for_uses_default_when_missing() {
    let c = ValidatorConfig::defaults();
    assert_eq!(c.weight_for("does_not_exist"), 1.0);
}

// ---------------------------------------------------------------------------
// pattern_key
// ---------------------------------------------------------------------------

#[test]
fn pattern_key_round_trip() {
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let key = pattern_key(&det);
    assert!(key.contains("harmonic:gartley_bull"));
    assert!(key.contains("H4"));
}

// ---------------------------------------------------------------------------
// RegimeAlignment channel
// ---------------------------------------------------------------------------

#[test]
fn regime_channel_favours_elliott_in_trend() {
    let ch = RegimeAlignment;
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::TrendingUp, TrendStrength::Strong),
    );
    let s = ch.evaluate(&det, &ValidationContext::default()).unwrap();
    assert!(s > 0.85, "got {s}");
}

#[test]
fn regime_channel_penalises_elliott_in_range() {
    let ch = RegimeAlignment;
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let s = ch.evaluate(&det, &ValidationContext::default()).unwrap();
    assert!(s < 0.5, "got {s}");
}

#[test]
fn regime_channel_favours_harmonic_in_range() {
    let ch = RegimeAlignment;
    let det = detection(
        PatternKind::Harmonic("bat_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let s = ch.evaluate(&det, &ValidationContext::default()).unwrap();
    assert!(s > 0.85, "got {s}");
}

// ---------------------------------------------------------------------------
// MultiTimeframeConfluence
// ---------------------------------------------------------------------------

#[test]
fn mtf_channel_returns_none_when_no_htf_supplied() {
    let ch = MultiTimeframeConfluence;
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H1,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    assert!(ch.evaluate(&det, &ValidationContext::default()).is_none());
}

#[test]
fn mtf_channel_scores_high_with_matching_htf() {
    let ch = MultiTimeframeConfluence;
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H1,
        0.7,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let mut ctx = ValidationContext::default();
    ctx.higher_tf_detections.push(detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.92,
        regime(RegimeKind::Ranging, TrendStrength::None),
    ));
    let s = ch.evaluate(&det, &ctx).unwrap();
    assert!((s - 0.92).abs() < 1e-3, "got {s}");
}

#[test]
fn mtf_channel_ignores_lower_or_unrelated_tf() {
    let ch = MultiTimeframeConfluence;
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.7,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let mut ctx = ValidationContext::default();
    // Lower TF — should be skipped.
    ctx.higher_tf_detections.push(detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H1,
        0.95,
        regime(RegimeKind::Ranging, TrendStrength::None),
    ));
    // Different family — should be skipped.
    ctx.higher_tf_detections.push(detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::D1,
        0.95,
        regime(RegimeKind::TrendingUp, TrendStrength::Strong),
    ));
    let s = ch.evaluate(&det, &ctx).unwrap();
    assert!(s < 0.5, "got {s}");
}

// ---------------------------------------------------------------------------
// HistoricalHitRate
// ---------------------------------------------------------------------------

#[test]
fn hit_rate_returns_none_below_min_samples() {
    let ch = HistoricalHitRate { min_samples: 30 };
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let mut ctx = ValidationContext::default();
    ctx.hit_rates.insert(
        pattern_key(&det),
        HitRateStat {
            samples: 5,
            hit_rate: 0.9,
        },
    );
    assert!(ch.evaluate(&det, &ctx).is_none());
}

#[test]
fn hit_rate_returns_value_above_min_samples() {
    let ch = HistoricalHitRate { min_samples: 30 };
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.8,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    let mut ctx = ValidationContext::default();
    ctx.hit_rates.insert(
        pattern_key(&det),
        HitRateStat {
            samples: 100,
            hit_rate: 0.62,
        },
    );
    let s = ch.evaluate(&det, &ctx).unwrap();
    assert!((s - 0.62).abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

fn full_validator() -> Validator {
    let mut v = Validator::new(ValidatorConfig::defaults()).unwrap();
    v.register(Arc::new(RegimeAlignment));
    v.register(Arc::new(MultiTimeframeConfluence));
    v.register(Arc::new(HistoricalHitRate { min_samples: 30 }));
    v
}

#[test]
fn validator_passes_strong_signal() {
    let v = full_validator();
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::H1,
        0.85,
        regime(RegimeKind::TrendingUp, TrendStrength::Strong),
    );
    let mut ctx = ValidationContext::default();
    ctx.higher_tf_detections.push(detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::H4,
        0.9,
        regime(RegimeKind::TrendingUp, TrendStrength::Strong),
    ));
    ctx.hit_rates.insert(
        pattern_key(&det),
        HitRateStat {
            samples: 200,
            hit_rate: 0.7,
        },
    );
    let validated = v.validate(det, &ctx).expect("should validate");
    assert!(validated.confidence > 0.7, "got {}", validated.confidence);
    // All three channels voted.
    assert_eq!(validated.channel_scores.len(), 3);
}

#[test]
fn validator_drops_weak_signal() {
    let v = full_validator();
    // Elliott impulse but in a ranging regime → regime channel hammers
    // it; structural score modest; no HTF; no hit rate.
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        Timeframe::H1,
        0.55,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    assert!(v.validate(det, &ValidationContext::default()).is_none());
}

#[test]
fn validator_excludes_silent_channels_from_blend() {
    let v = full_validator();
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        Timeframe::H4,
        0.95,
        regime(RegimeKind::Ranging, TrendStrength::None),
    );
    // No HTF, no hit rates → only regime_alignment votes. Confidence
    // should still clear because the structural score is excellent and
    // regime alignment for harmonic in a range is also high.
    let validated = v.validate(det, &ValidationContext::default()).expect("validate");
    assert_eq!(validated.channel_scores.len(), 1);
    assert_eq!(validated.channel_scores[0].channel, "regime_alignment");
    assert!(validated.confidence > 0.8, "got {}", validated.confidence);
}

#[test]
fn validator_register_increments_channel_count() {
    let mut v = Validator::new(ValidatorConfig::defaults()).unwrap();
    assert_eq!(v.channel_count(), 0);
    v.register(Arc::new(RegimeAlignment));
    assert_eq!(v.channel_count(), 1);
}
