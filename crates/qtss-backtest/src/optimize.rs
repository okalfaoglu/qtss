use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, VecDeque};

use qtss_domain::bar::TimestampBar;
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;

use crate::engine::{BacktestConfig, BacktestEngine, BacktestResult};
use crate::strategy::Strategy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardConfig {
    pub train_bars: usize,
    pub test_bars: usize,
    pub step_bars: usize,
}

impl Default for WalkForwardConfig {
    fn default() -> Self {
        Self {
            train_bars: 2_000,
            test_bars: 500,
            step_bars: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParameterGrid {
    pub axes: BTreeMap<String, Vec<JsonValue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub best_params: BTreeMap<String, JsonValue>,
    pub score: Decimal,
    pub walk_forward_summary: String,
    pub runs: Vec<BacktestResult>,
}

pub struct Optimizer {
    pub walk_forward: WalkForwardConfig,
}

impl Optimizer {
    /// Parametre ızgarası + walk-forward ortalama skoru. Her WF penceresi **yeni** strateji örneği ile çalışır.
    pub fn grid_search<S: Strategy>(
        &self,
        engine_cfg: BacktestConfig,
        instrument: InstrumentId,
        bars: &[TimestampBar],
        grid: ParameterGrid,
        mut strategy_factory: impl FnMut(&BTreeMap<String, JsonValue>) -> S,
        score_fn: impl Fn(&BacktestResult) -> Decimal,
    ) -> OptimizationResult {
        let combos = cartesian_product(&grid.axes);
        let mut best_score = Decimal::MIN;
        let mut best_params = BTreeMap::new();
        let mut all_runs = Vec::new();

        for params in combos {
            let wf_score = self.walk_forward_avg(
                &engine_cfg,
                instrument.clone(),
                bars,
                &params,
                &mut strategy_factory,
                &score_fn,
                &mut all_runs,
            );
            if wf_score > best_score {
                best_score = wf_score;
                best_params = params;
            }
        }

        OptimizationResult {
            best_params,
            score: best_score,
            walk_forward_summary: format!(
                "train={} test={} step={}",
                self.walk_forward.train_bars,
                self.walk_forward.test_bars,
                self.walk_forward.step_bars
            ),
            runs: all_runs,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_forward_avg<S: Strategy>(
        &self,
        engine_cfg: &BacktestConfig,
        instrument: InstrumentId,
        bars: &[TimestampBar],
        params: &BTreeMap<String, JsonValue>,
        strategy_factory: &mut impl FnMut(&BTreeMap<String, JsonValue>) -> S,
        score_fn: &impl Fn(&BacktestResult) -> Decimal,
        acc: &mut Vec<BacktestResult>,
    ) -> Decimal {
        let wf = &self.walk_forward;
        if bars.len() < wf.train_bars + wf.test_bars {
            let mut strategy = strategy_factory(params);
            let eng = BacktestEngine::new(engine_cfg.clone());
            let q: VecDeque<_> = bars.iter().cloned().collect();
            let res = eng.run(instrument, q, &mut strategy);
            let s = score_fn(&res);
            acc.push(res);
            return s;
        }

        let mut agg = Decimal::ZERO;
        let mut windows = 0usize;
        let mut start = 0usize;
        while start + wf.train_bars + wf.test_bars <= bars.len() {
            let test_start = start + wf.train_bars;
            let test_end = test_start + wf.test_bars;
            let slice: VecDeque<_> = bars[test_start..test_end].iter().cloned().collect();
            let mut strategy = strategy_factory(params);
            let eng = BacktestEngine::new(engine_cfg.clone());
            let res = eng.run(instrument.clone(), slice, &mut strategy);
            agg += score_fn(&res);
            acc.push(res);
            windows += 1;
            start += wf.step_bars;
        }
        if windows == 0 {
            return Decimal::ZERO;
        }
        agg / Decimal::from(windows as u64)
    }
}

fn cartesian_product(axes: &BTreeMap<String, Vec<JsonValue>>) -> Vec<BTreeMap<String, JsonValue>> {
    if axes.is_empty() {
        return vec![BTreeMap::new()];
    }
    let mut levels: Vec<(&String, &[JsonValue])> =
        axes.iter().map(|(k, v)| (k, v.as_slice())).collect();
    levels.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = vec![BTreeMap::new()];
    for (k, vals) in levels {
        let mut next = Vec::new();
        for base in &out {
            for v in vals {
                let mut m = base.clone();
                m.insert((*k).clone(), v.clone());
                next.push(m);
            }
        }
        out = next;
    }
    out
}
