//! `/v2/montecarlo/{venue}/{symbol}/{tf}` wire types -- Faz 5 Adim (e).
//!
//! The Monte Carlo Fan panel projects a cone of plausible price paths
//! forward from the current bar so the trader can eyeball "where could
//! we be in N bars" instead of staring at a single point estimate.
//!
//! The wire shape is a list of percentile bands (one entry per
//! percentile, e.g. p05/p25/p50/p75/p95) where each band carries the
//! price at every step of the horizon. This is exactly what a fan
//! chart needs: the renderer fills the area between symmetric bands
//! and draws a center line through the median.
//!
//! ## Determinism
//!
//! The simulator is seeded so the same input window always yields the
//! same fan -- screenshots in the dashboard stay reproducible and
//! tests can assert exact percentile values without flake. The seed is
//! a route parameter (default 0) so a user can re-roll on demand.
//!
//! ## Self-contained RNG
//!
//! We deliberately avoid pulling `rand` / `rand_distr` into this
//! contract crate. The simulator uses a SplitMix64 PRNG plus a
//! Box-Muller normal generator -- both are <30 lines and have no
//! external dependencies, which keeps the wire crate light.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One percentile band over the horizon. `values[i]` is the price at
/// step `i+1` (step 0 = anchor and is implicit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanBand {
    /// Percentile in 0..=100 (e.g. 5 for p05, 50 for the median).
    pub percentile: u8,
    pub values: Vec<Decimal>,
}

/// Whole `/v2/montecarlo/...` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloFan {
    pub generated_at: DateTime<Utc>,
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub horizon_bars: u32,
    pub paths_simulated: u32,
    pub anchor_price: Decimal,
    /// Sorted ascending by `percentile`. Always contains the median.
    pub bands: Vec<FanBand>,
}

/// Pure builder used by the route handler. Calibrates a geometric
/// brownian motion to the log-return mean / stdev of the input
/// closes, simulates `paths` independent trajectories of length
/// `horizon`, and emits the requested percentile bands per step.
pub fn build_montecarlo_fan(
    closes: &[Decimal],
    anchor: Decimal,
    horizon_bars: u32,
    paths: u32,
    percentiles: &[u8],
    seed: u64,
) -> Vec<FanBand> {
    let horizon = horizon_bars as usize;
    let n_paths = paths.max(1) as usize;
    if horizon == 0 || percentiles.is_empty() {
        return Vec::new();
    }

    let (mu, sigma) = log_return_stats(closes);
    let anchor_f = decimal_to_f64(&anchor).unwrap_or(0.0);

    // Simulate. `step_buf[t]` will hold every path's price at step t.
    let mut step_buf: Vec<Vec<f64>> = vec![Vec::with_capacity(n_paths); horizon];
    let mut rng = SplitMix64::new(seed);
    for _ in 0..n_paths {
        let mut price = anchor_f;
        for slot in step_buf.iter_mut() {
            let z = rng.next_normal();
            // GBM increment: r = (mu - 0.5 sigma^2) + sigma * z.
            let r = (mu - 0.5 * sigma * sigma) + sigma * z;
            price *= r.exp();
            slot.push(price);
        }
    }

    let mut sorted: Vec<u8> = percentiles.iter().copied().collect();
    sorted.sort_unstable();
    sorted.dedup();

    sorted
        .into_iter()
        .map(|p| FanBand {
            percentile: p,
            values: step_buf
                .iter_mut()
                .map(|slot| {
                    slot.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    decimal_from_f64(percentile_of_sorted(slot, p))
                })
                .collect(),
        })
        .collect()
}

fn log_return_stats(closes: &[Decimal]) -> (f64, f64) {
    if closes.len() < 2 {
        return (0.0, 0.0);
    }
    let f: Vec<f64> = closes.iter().filter_map(decimal_to_f64).collect();
    if f.len() < 2 {
        return (0.0, 0.0);
    }
    let returns: Vec<f64> = f
        .windows(2)
        .filter_map(|w| {
            if w[0] > 0.0 && w[1] > 0.0 {
                Some((w[1] / w[0]).ln())
            } else {
                None
            }
        })
        .collect();
    if returns.is_empty() {
        return (0.0, 0.0);
    }
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let var: f64 =
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    (mean, var.sqrt())
}

/// Linear-interpolation percentile (sorted ascending).
fn percentile_of_sorted(sorted: &[f64], p: u8) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let frac = (p as f64 / 100.0).clamp(0.0, 1.0);
    let pos = frac * (sorted.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let w = pos - lo as f64;
        sorted[lo] * (1.0 - w) + sorted[hi] * w
    }
}

/// SplitMix64 PRNG. Tiny, fast, and good enough for visualization
/// fans -- not cryptographically secure (and that's fine, we're
/// drawing pictures).
struct SplitMix64 {
    state: u64,
    spare: Option<f64>,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        // Avoid the all-zero degenerate state.
        let s = if seed == 0 { 0x9E3779B97F4A7C15 } else { seed };
        Self { state: s, spare: None }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn next_f64(&mut self) -> f64 {
        // 53-bit uniform in [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Box-Muller normal. Returns one sample per call, caching its
    /// pair for the next call.
    fn next_normal(&mut self) -> f64 {
        if let Some(s) = self.spare.take() {
            return s;
        }
        // Avoid log(0).
        let u1 = (self.next_f64()).max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }
}

fn decimal_to_f64(d: &Decimal) -> Option<f64> {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64()
}

fn decimal_from_f64(f: f64) -> Decimal {
    use rust_decimal::prelude::FromPrimitive;
    Decimal::from_f64(f).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn closes_drifting_up() -> Vec<Decimal> {
        (0..50)
            .map(|i| decimal_from_f64(100.0 * (1.0 + i as f64 * 0.01)))
            .collect()
    }

    #[test]
    fn fan_has_one_band_per_requested_percentile() {
        let closes = closes_drifting_up();
        let bands =
            build_montecarlo_fan(&closes, dec!(150), 20, 200, &[5, 25, 50, 75, 95], 42);
        assert_eq!(bands.len(), 5);
        let pcts: Vec<u8> = bands.iter().map(|b| b.percentile).collect();
        assert_eq!(pcts, vec![5, 25, 50, 75, 95]);
        for b in &bands {
            assert_eq!(b.values.len(), 20);
        }
    }

    #[test]
    fn percentiles_are_monotone_at_each_step() {
        let closes = closes_drifting_up();
        let bands =
            build_montecarlo_fan(&closes, dec!(150), 15, 500, &[10, 50, 90], 7);
        for step in 0..15 {
            let p10 = bands[0].values[step];
            let p50 = bands[1].values[step];
            let p90 = bands[2].values[step];
            assert!(p10 <= p50, "p10 > p50 at step {step}: {p10} > {p50}");
            assert!(p50 <= p90, "p50 > p90 at step {step}: {p50} > {p90}");
        }
    }

    #[test]
    fn deterministic_with_same_seed() {
        let closes = closes_drifting_up();
        let a = build_montecarlo_fan(&closes, dec!(150), 10, 100, &[50], 1234);
        let b = build_montecarlo_fan(&closes, dec!(150), 10, 100, &[50], 1234);
        assert_eq!(a, b);
    }

    #[test]
    fn flat_series_yields_anchor_band() {
        let closes: Vec<Decimal> = vec![dec!(100); 30];
        let bands = build_montecarlo_fan(&closes, dec!(100), 5, 50, &[50], 99);
        // Zero variance -> every path stays at the anchor.
        for v in &bands[0].values {
            assert_eq!(*v, dec!(100));
        }
    }

    #[test]
    fn empty_horizon_yields_no_bands() {
        let closes = closes_drifting_up();
        let bands = build_montecarlo_fan(&closes, dec!(150), 0, 100, &[50], 1);
        assert!(bands.is_empty());
    }

    #[test]
    fn json_round_trip() {
        let fan = MonteCarloFan {
            generated_at: Utc::now(),
            venue: "binance".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            horizon_bars: 5,
            paths_simulated: 100,
            anchor_price: dec!(150),
            bands: build_montecarlo_fan(&closes_drifting_up(), dec!(150), 5, 100, &[50], 0),
        };
        let j = serde_json::to_string(&fan).unwrap();
        let back: MonteCarloFan = serde_json::from_str(&j).unwrap();
        assert_eq!(back.bands.len(), 1);
        assert_eq!(back.bands[0].values.len(), 5);
    }
}
