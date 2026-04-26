//! Optimisation framework — grid search + walk-forward + simple
//! random search over the 10 composite weights.
//!
//! Pipeline:
//!
//!   GridSpec    -> WeightSweep   -> [N CompositeWeights configs]
//!                                   ↓
//!   WalkForwardSpec(in_sample, oos_sample, slide_step)
//!                                   ↓
//!   For each (config, window): run IqBacktestRunner; capture
//!     IqBacktestReport on BOTH in-sample and out-of-sample slices.
//!                                   ↓
//!   Rank by oos_score (avoids in-sample overfitting).
//!                                   ↓
//!   Sensitivity report: per-weight delta-vs-baseline impact.
//!
//! NOT included in this commit (parked for 26.5):
//!   - Bayesian optimisation (the surrogate-model loop). Random
//!     search + grid is enough for the first sweep over BTC 4h.
//!   - Parallel execution. The runner is async; in 26.4 we run
//!     configs sequentially. `tokio::task::JoinSet` parallelisation
//!     lands when the runner is provably IO-bound (current bottleneck
//!     is detection lookup per bar, ~1ms/bar — single-threaded fine
//!     for low-thousand-bar windows).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{info, warn};

use super::config::{CompositeWeights, IqBacktestConfig};
use super::report::IqBacktestReport;
use super::runner::IqBacktestRunner;

/// Range over a single weight channel — used by GridSpec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightRange {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl WeightRange {
    /// Discretise the range into a vector of values, inclusive of
    /// both endpoints. Step <= 0 yields just `[min]`.
    pub fn enumerate(&self) -> Vec<f64> {
        if self.step <= 0.0 || self.max < self.min {
            return vec![self.min];
        }
        let mut out = Vec::new();
        let mut v = self.min;
        // Use a small epsilon so float drift doesn't drop the
        // upper bound.
        while v <= self.max + 1e-9 {
            out.push(v);
            v += self.step;
        }
        out
    }
}

/// Grid specification — which channels to sweep, with what range.
/// Channels NOT in the spec stay at the baseline value taken from
/// `IqBacktestConfig.weights`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GridSpec {
    pub structural: Option<WeightRange>,
    pub fib_retrace: Option<WeightRange>,
    pub volume_capit: Option<WeightRange>,
    pub cvd_divergence: Option<WeightRange>,
    pub indicator: Option<WeightRange>,
    pub sentiment: Option<WeightRange>,
    pub multi_tf: Option<WeightRange>,
    pub funding_oi: Option<WeightRange>,
    pub wyckoff_alignment: Option<WeightRange>,
    pub cycle_alignment: Option<WeightRange>,
    /// Renormalise sums to this target after sweeping. None = leave
    /// raw. Live default is 1.00.
    pub normalise_to: Option<f64>,
}

impl GridSpec {
    /// Cartesian product over the configured channels. Returns a
    /// vector of fully-specified `CompositeWeights` ready to drop
    /// into a backtest.
    pub fn enumerate(&self, baseline: &CompositeWeights) -> Vec<CompositeWeights> {
        macro_rules! axis {
            ($name:ident) => {
                self.$name
                    .as_ref()
                    .map(|r| r.enumerate())
                    .unwrap_or_else(|| vec![baseline.$name])
            };
        }
        let s = axis!(structural);
        let f = axis!(fib_retrace);
        let v = axis!(volume_capit);
        let c = axis!(cvd_divergence);
        let i = axis!(indicator);
        let se = axis!(sentiment);
        let m = axis!(multi_tf);
        let fu = axis!(funding_oi);
        let w = axis!(wyckoff_alignment);
        let cy = axis!(cycle_alignment);

        let mut out = Vec::with_capacity(
            s.len() * f.len() * v.len() * c.len() * i.len()
                * se.len() * m.len() * fu.len() * w.len() * cy.len(),
        );
        for &sv in &s {
            for &fv in &f {
                for &vv in &v {
                    for &cv in &c {
                        for &iv in &i {
                            for &sev in &se {
                                for &mv in &m {
                                    for &fuv in &fu {
                                        for &wv in &w {
                                            for &cyv in &cy {
                                                let cfg = CompositeWeights {
                                                    structural: sv,
                                                    fib_retrace: fv,
                                                    volume_capit: vv,
                                                    cvd_divergence: cv,
                                                    indicator: iv,
                                                    sentiment: sev,
                                                    multi_tf: mv,
                                                    funding_oi: fuv,
                                                    wyckoff_alignment: wv,
                                                    cycle_alignment: cyv,
                                                };
                                                let cfg = match self.normalise_to {
                                                    Some(t) => cfg.normalised(t),
                                                    None => cfg,
                                                };
                                                out.push(cfg);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

/// Walk-forward window splitting. Each window has an in-sample
/// training slice and an out-of-sample test slice; we slide the
/// pair forward by `slide_step` until the dataset is exhausted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkForwardSpec {
    pub in_sample: Duration,
    pub out_of_sample: Duration,
    pub slide_step: Duration,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

impl WalkForwardSpec {
    /// Generate every (in_sample_start..in_sample_end,
    /// oos_start..oos_end) pair that fits inside [start_at, end_at].
    pub fn windows(&self) -> Vec<WalkForwardWindow> {
        let mut out = Vec::new();
        let mut t = self.start_at;
        while t + self.in_sample + self.out_of_sample <= self.end_at {
            let is_start = t;
            let is_end = t + self.in_sample;
            let oos_start = is_end;
            let oos_end = oos_start + self.out_of_sample;
            out.push(WalkForwardWindow {
                in_sample_start: is_start,
                in_sample_end: is_end,
                oos_start,
                oos_end,
            });
            t = t + self.slide_step;
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkForwardWindow {
    pub in_sample_start: DateTime<Utc>,
    pub in_sample_end: DateTime<Utc>,
    pub oos_start: DateTime<Utc>,
    pub oos_end: DateTime<Utc>,
}

/// Single optimisation result row — one config, one window, both
/// in-sample and out-of-sample reports + the rank score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub weights: CompositeWeights,
    pub window: WalkForwardWindow,
    pub in_sample: IqBacktestReport,
    pub out_of_sample: IqBacktestReport,
    /// Rank score uses OOS to avoid overfitting. Default formula:
    /// `oos.score()`.
    pub rank_score: f64,
}

/// Aggregate across all windows for one weight config — the
/// summary the report ranks weight configs by.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSummary {
    pub weights: CompositeWeights,
    pub mean_in_sample_score: f64,
    pub mean_oos_score: f64,
    pub stddev_oos_score: f64,
    pub windows_evaluated: u32,
    /// `mean_oos_score / mean_in_sample_score`. > 0.7 = robust;
    /// < 0.5 = overfit.
    pub robustness_ratio: f64,
}

/// Per-channel sensitivity diagnostic — how does varying ONE
/// weight channel (with all others fixed at baseline) impact OOS
/// score? Computed as the linear correlation between the weight
/// value and oos score across all configs that vary only that
/// channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityRow {
    pub channel: String,
    pub correlation_with_oos: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub best_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationReport {
    pub configs_evaluated: u32,
    pub windows_evaluated: u32,
    /// Top configs ranked by mean OOS score, descending.
    pub leaderboard: Vec<ConfigSummary>,
    pub sensitivity: Vec<SensitivityRow>,
}

/// Optimisation runner. Hands every (config, window) pair to a
/// freshly-built `IqBacktestRunner` and collects results.
pub struct OptimizationRunner {
    pub base_config: IqBacktestConfig,
    pub grid: GridSpec,
    pub walk_forward: WalkForwardSpec,
    /// Concurrency for the parallel `run_parallel`. Defaults to 4
    /// — sensible for an 8-conn pool. Cap at the pool size or DB
    /// will starve.
    pub max_concurrency: usize,
}

impl OptimizationRunner {
    pub fn new(
        base_config: IqBacktestConfig,
        grid: GridSpec,
        walk_forward: WalkForwardSpec,
    ) -> Self {
        Self {
            base_config,
            grid,
            walk_forward,
            max_concurrency: 4,
        }
    }

    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = n.max(1);
        self
    }

    /// Sequential execution. Kept for determinism + small runs;
    /// large sweeps should use `run_parallel`.
    pub async fn run(&self, pool: &PgPool) -> anyhow::Result<OptimizationReport> {
        let configs = self.grid.enumerate(&self.base_config.weights);
        let windows = self.walk_forward.windows();
        info!(
            configs = configs.len(),
            windows = windows.len(),
            total_runs = configs.len() * windows.len() * 2,
            "iq-optimize starting (sequential)"
        );

        let mut results: Vec<OptimizationResult> = Vec::new();
        for (ci, weights) in configs.iter().enumerate() {
            for (wi, window) in windows.iter().enumerate() {
                let r = self.run_one(pool, ci, wi, weights, window).await?;
                results.push(r);
            }
        }
        Ok(self.aggregate(&results))
    }

    /// FAZ 26.5 finish — parallel execution. Spawns up to
    /// `max_concurrency` (config × window) tasks at a time on a
    /// `JoinSet`. The DB pool is the actual concurrency cap; never
    /// set max_concurrency > pool.max_connections.
    pub async fn run_parallel(
        &self,
        pool: &PgPool,
    ) -> anyhow::Result<OptimizationReport> {
        let configs = self.grid.enumerate(&self.base_config.weights);
        let windows = self.walk_forward.windows();
        info!(
            configs = configs.len(),
            windows = windows.len(),
            total_runs = configs.len() * windows.len() * 2,
            concurrency = self.max_concurrency,
            "iq-optimize starting (parallel)"
        );

        let pool = pool.clone();
        let base_config = Arc::new(self.base_config.clone());
        let mut set: JoinSet<anyhow::Result<OptimizationResult>> =
            JoinSet::new();
        let mut completed: Vec<OptimizationResult> = Vec::new();
        let mut idx = 0usize;
        let total = configs.len() * windows.len();

        // Pre-build the (config, window) work queue.
        type Work = (usize, usize, CompositeWeights, WalkForwardWindow);
        let work: Vec<Work> = configs
            .iter()
            .enumerate()
            .flat_map(|(ci, w)| {
                windows.iter().enumerate().map(move |(wi, win)| {
                    (ci, wi, w.clone(), win.clone())
                })
            })
            .collect();

        // Prime the join set up to max_concurrency.
        while idx < work.len() && set.len() < self.max_concurrency {
            let (ci, wi, weights, window) = work[idx].clone();
            let pool_c = pool.clone();
            let base_c = base_config.clone();
            set.spawn(async move {
                run_window(pool_c, base_c, ci, wi, weights, window).await
            });
            idx += 1;
        }

        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(r)) => completed.push(r),
                Ok(Err(e)) => warn!(%e, "optimization task failed"),
                Err(e) => warn!(%e, "optimization task panicked"),
            }
            if idx < work.len() {
                let (ci, wi, weights, window) = work[idx].clone();
                let pool_c = pool.clone();
                let base_c = base_config.clone();
                set.spawn(async move {
                    run_window(pool_c, base_c, ci, wi, weights, window).await
                });
                idx += 1;
            }
            if completed.len() % 10 == 0 {
                info!(
                    completed = completed.len(),
                    total = total,
                    "iq-optimize progress"
                );
            }
        }

        Ok(self.aggregate(&completed))
    }

    async fn run_one(
        &self,
        pool: &PgPool,
        ci: usize,
        wi: usize,
        weights: &CompositeWeights,
        window: &WalkForwardWindow,
    ) -> anyhow::Result<OptimizationResult> {
        let mut is_cfg = self.base_config.clone();
        is_cfg.weights = weights.clone();
        is_cfg.universe.start_time = window.in_sample_start;
        is_cfg.universe.end_time = window.in_sample_end;
        is_cfg.run_tag = format!("opt-c{}-w{}-is", ci, wi);
        let is_runner = IqBacktestRunner::new(is_cfg)?;
        let is_report = is_runner.run(pool).await?;

        let mut oos_cfg = self.base_config.clone();
        oos_cfg.weights = weights.clone();
        oos_cfg.universe.start_time = window.oos_start;
        oos_cfg.universe.end_time = window.oos_end;
        oos_cfg.run_tag = format!("opt-c{}-w{}-oos", ci, wi);
        let oos_runner = IqBacktestRunner::new(oos_cfg)?;
        let oos_report = oos_runner.run(pool).await?;

        let rank = oos_report.score();
        Ok(OptimizationResult {
            weights: weights.clone(),
            window: window.clone(),
            in_sample: is_report,
            out_of_sample: oos_report,
            rank_score: rank,
        })
    }

    /// FAZ 26.5 finish — random search. Samples N random points
    /// from the grid, evaluates each against every walk-forward
    /// window. Useful when the cartesian product is too large.
    pub async fn run_random(
        &self,
        pool: &PgPool,
        sample_n: usize,
        seed: u64,
    ) -> anyhow::Result<OptimizationReport> {
        let configs = self.grid.enumerate(&self.base_config.weights);
        let windows = self.walk_forward.windows();
        let n = sample_n.min(configs.len());
        // Linear-congruential PRNG seeded by `seed` — deterministic
        // sampling so a "this seed gave best result" replay works.
        let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let mut indices: Vec<usize> = (0..configs.len()).collect();
        for i in (1..indices.len()).rev() {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let j = (state as usize) % (i + 1);
            indices.swap(i, j);
        }
        info!(
            sampled = n,
            from_grid = configs.len(),
            windows = windows.len(),
            "iq-optimize starting (random search)"
        );
        let mut results: Vec<OptimizationResult> = Vec::new();
        for (ci_out, ci) in indices.iter().take(n).enumerate() {
            for (wi, window) in windows.iter().enumerate() {
                let r = self
                    .run_one(pool, ci_out, wi, &configs[*ci], window)
                    .await?;
                results.push(r);
            }
        }
        Ok(self.aggregate(&results))
    }

    fn aggregate(&self, results: &[OptimizationResult]) -> OptimizationReport {
        // Group by config (weights) — then average across windows.
        // We use a stable JSON serialisation as the grouping key so
        // float-precision drift doesn't fragment buckets.
        use std::collections::BTreeMap;
        let mut by_cfg: BTreeMap<String, Vec<&OptimizationResult>> = BTreeMap::new();
        for r in results {
            let key = serde_json::to_string(&r.weights).unwrap_or_default();
            by_cfg.entry(key).or_default().push(r);
        }
        let mut leaderboard: Vec<ConfigSummary> = by_cfg
            .into_iter()
            .map(|(_, group)| {
                let n = group.len() as f64;
                let mean_is = group.iter().map(|r| r.in_sample.score()).sum::<f64>() / n;
                let mean_oos = group.iter().map(|r| r.out_of_sample.score()).sum::<f64>() / n;
                let var_oos = group
                    .iter()
                    .map(|r| (r.out_of_sample.score() - mean_oos).powi(2))
                    .sum::<f64>()
                    / n;
                let stddev_oos = var_oos.sqrt();
                let robustness = if mean_is.abs() > 1e-9 {
                    mean_oos / mean_is
                } else {
                    0.0
                };
                ConfigSummary {
                    weights: group[0].weights.clone(),
                    mean_in_sample_score: mean_is,
                    mean_oos_score: mean_oos,
                    stddev_oos_score: stddev_oos,
                    windows_evaluated: group.len() as u32,
                    robustness_ratio: robustness,
                }
            })
            .collect();
        leaderboard
            .sort_by(|a, b| b.mean_oos_score.partial_cmp(&a.mean_oos_score).unwrap_or(std::cmp::Ordering::Equal));

        let sensitivity = self.sensitivity(&leaderboard);
        OptimizationReport {
            configs_evaluated: leaderboard.len() as u32,
            windows_evaluated: results
                .iter()
                .map(|r| {
                    serde_json::to_string(&r.window).unwrap_or_default()
                })
                .collect::<std::collections::BTreeSet<_>>()
                .len() as u32,
            leaderboard,
            sensitivity,
        }
    }

    /// For each channel, correlation between (weight value, mean
    /// OOS score) across the leaderboard. Pearson r — NaN when the
    /// channel has no variance.
    fn sensitivity(&self, leaderboard: &[ConfigSummary]) -> Vec<SensitivityRow> {
        if leaderboard.is_empty() {
            return Vec::new();
        }
        let extract: Vec<(&str, fn(&CompositeWeights) -> f64)> = vec![
            ("structural", |w| w.structural),
            ("fib_retrace", |w| w.fib_retrace),
            ("volume_capit", |w| w.volume_capit),
            ("cvd_divergence", |w| w.cvd_divergence),
            ("indicator", |w| w.indicator),
            ("sentiment", |w| w.sentiment),
            ("multi_tf", |w| w.multi_tf),
            ("funding_oi", |w| w.funding_oi),
            ("wyckoff_alignment", |w| w.wyckoff_alignment),
            ("cycle_alignment", |w| w.cycle_alignment),
        ];
        let mut out = Vec::new();
        for (name, getter) in extract {
            let xs: Vec<f64> =
                leaderboard.iter().map(|c| getter(&c.weights)).collect();
            let ys: Vec<f64> =
                leaderboard.iter().map(|c| c.mean_oos_score).collect();
            let r = pearson(&xs, &ys);
            let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
            let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let best = leaderboard
                .first()
                .map(|c| getter(&c.weights))
                .unwrap_or(0.0);
            out.push(SensitivityRow {
                channel: name.into(),
                correlation_with_oos: r,
                min_value: min,
                max_value: max,
                best_value: best,
            });
        }
        out
    }
}

/// Free function for parallel spawning — `IqBacktestRunner` is not
/// `Sync` (owns a JSONL writer mutex), so we build a fresh one
/// inside each task.
async fn run_window(
    pool: PgPool,
    base_config: Arc<IqBacktestConfig>,
    ci: usize,
    wi: usize,
    weights: CompositeWeights,
    window: WalkForwardWindow,
) -> anyhow::Result<OptimizationResult> {
    let mut is_cfg = (*base_config).clone();
    is_cfg.weights = weights.clone();
    is_cfg.universe.start_time = window.in_sample_start;
    is_cfg.universe.end_time = window.in_sample_end;
    is_cfg.run_tag = format!("opt-c{}-w{}-is", ci, wi);
    let is_report = IqBacktestRunner::new(is_cfg)?.run(&pool).await?;

    let mut oos_cfg = (*base_config).clone();
    oos_cfg.weights = weights.clone();
    oos_cfg.universe.start_time = window.oos_start;
    oos_cfg.universe.end_time = window.oos_end;
    oos_cfg.run_tag = format!("opt-c{}-w{}-oos", ci, wi);
    let oos_report = IqBacktestRunner::new(oos_cfg)?.run(&pool).await?;

    let rank = oos_report.score();
    Ok(OptimizationResult {
        weights,
        window,
        in_sample: is_report,
        out_of_sample: oos_report,
        rank_score: rank,
    })
}

fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
    if xs.len() != ys.len() || xs.len() < 2 {
        return f64::NAN;
    }
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut dx = 0.0;
    let mut dy = 0.0;
    for i in 0..xs.len() {
        let ex = xs[i] - mx;
        let ey = ys[i] - my;
        num += ex * ey;
        dx += ex * ex;
        dy += ey * ey;
    }
    let den = (dx * dy).sqrt();
    if den < 1e-12 {
        f64::NAN
    } else {
        num / den
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::config::IqBacktestConfig;
    use chrono::TimeZone;

    #[test]
    fn weight_range_enumerate_inclusive() {
        let r = WeightRange { min: 0.10, max: 0.20, step: 0.05 };
        let v = r.enumerate();
        assert_eq!(v.len(), 3);
        assert!((v[0] - 0.10).abs() < 1e-9);
        assert!((v[2] - 0.20).abs() < 1e-9);
    }

    #[test]
    fn weight_range_zero_step_returns_min() {
        let r = WeightRange { min: 0.5, max: 0.7, step: 0.0 };
        let v = r.enumerate();
        assert_eq!(v, vec![0.5]);
    }

    #[test]
    fn grid_enumerate_cartesian_product() {
        let baseline = CompositeWeights::default();
        let grid = GridSpec {
            structural: Some(WeightRange {
                min: 0.10,
                max: 0.20,
                step: 0.05,
            }),
            wyckoff_alignment: Some(WeightRange {
                min: 0.10,
                max: 0.20,
                step: 0.05,
            }),
            ..Default::default()
        };
        let configs = grid.enumerate(&baseline);
        assert_eq!(configs.len(), 9); // 3 × 3
    }

    #[test]
    fn grid_normalises_to_one() {
        let baseline = CompositeWeights::default();
        let grid = GridSpec {
            structural: Some(WeightRange {
                min: 0.30,
                max: 0.30,
                step: 0.0,
            }),
            normalise_to: Some(1.0),
            ..Default::default()
        };
        let configs = grid.enumerate(&baseline);
        assert_eq!(configs.len(), 1);
        let sum = configs[0].sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum={sum}");
    }

    #[test]
    fn walk_forward_windows_slide_correctly() {
        let start = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = chrono::Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        let wf = WalkForwardSpec {
            in_sample: Duration::days(30),
            out_of_sample: Duration::days(15),
            slide_step: Duration::days(15),
            start_at: start,
            end_at: end,
        };
        let windows = wf.windows();
        assert!(windows.len() >= 3);
        assert_eq!(windows[0].in_sample_start, start);
        assert_eq!(windows[0].in_sample_end, start + Duration::days(30));
    }

    #[test]
    fn pearson_perfect_positive_returns_one() {
        let xs = vec![1.0, 2.0, 3.0, 4.0];
        let ys = vec![2.0, 4.0, 6.0, 8.0];
        let r = pearson(&xs, &ys);
        assert!((r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pearson_perfect_negative_returns_minus_one() {
        let xs = vec![1.0, 2.0, 3.0, 4.0];
        let ys = vec![8.0, 6.0, 4.0, 2.0];
        let r = pearson(&xs, &ys);
        assert!((r - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn pearson_no_variance_returns_nan() {
        let xs = vec![1.0, 1.0, 1.0];
        let ys = vec![5.0, 6.0, 7.0];
        let r = pearson(&xs, &ys);
        assert!(r.is_nan());
    }

    #[test]
    fn optimization_runner_constructs() {
        let cfg = IqBacktestConfig::default();
        let grid = GridSpec::default();
        let wf = WalkForwardSpec {
            in_sample: Duration::days(30),
            out_of_sample: Duration::days(15),
            slide_step: Duration::days(15),
            start_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            end_at: chrono::Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        };
        let _ = OptimizationRunner::new(cfg, grid, wf);
    }

    #[test]
    fn optimization_with_concurrency_clamps_to_one() {
        let cfg = IqBacktestConfig::default();
        let grid = GridSpec::default();
        let wf = WalkForwardSpec {
            in_sample: Duration::days(30),
            out_of_sample: Duration::days(15),
            slide_step: Duration::days(15),
            start_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            end_at: chrono::Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        };
        let r = OptimizationRunner::new(cfg, grid, wf).with_concurrency(0);
        assert_eq!(r.max_concurrency, 1);
    }
}

// chrono::TimeZone used only inside #[cfg(test)] blocks; explicit
// import there.
