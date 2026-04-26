# IQ-D / IQ-T Backtest Module — Operator Guide

FAZ 26 enterprise backtest pipeline. Replays the live worker's
IQ-D (long) / IQ-T (short) candidate detection over historical bars,
captures every trade, classifies losses, and produces a rich report
suitable for weight optimisation + post-mortem.

## Why this module exists

The live worker decides "is this a Major Dip?" by computing a
10-channel composite score (structural / fib / volume / CVD /
indicator / sentiment / multi-TF / funding / Wyckoff event alignment /
macro cycle context) at every bar tick. If composite ≥ threshold
AND polarity gates pass, it spawns an IQ-D / IQ-T setup.

Backtest replays the SAME logic over historical bars, with one
critical guarantee: **no future leakage**. Every detection query
is bounded by `WHERE end_time <= bar_time`, so a setup fired at
bar N only sees data the live worker would have had at bar N.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    qtss-backtest::iq                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  IqBacktestConfig         (universe + weights + gates + risk)   │
│       │                                                         │
│       ▼                                                         │
│  IqBacktestRunner ─── IqReplayDetector  (10-channel scorers)    │
│       │                     │                                   │
│       │                     ├── scorers.rs (8 time-cutoff fns)  │
│       │                     ├── wyckoff alignment matrix        │
│       │                     └── cycle alignment matrix          │
│       │                                                         │
│       ├── IqLifecycleManager (TP ladder + SL + trailing + tmo)  │
│       │                                                         │
│       ├── CostModel (fee + slippage + funding)                  │
│       │                                                         │
│       └── attribution::classify (per-trade outcome reason)      │
│              │                                                  │
│              ▼                                                  │
│           TradeLogWriter (JSONL append-only)                    │
│              │                                                  │
│              ▼                                                  │
│           IqBacktestReport (PnL + win-rate + loss-reason +      │
│                              avg-loss-components)               │
│                                                                 │
│  OptimizationRunner (grid search × walk-forward)                │
│       └── ranks weight configs by mean OOS score                │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Quick start

```bash
# 1. Populate market_bars + detections (run the live worker for the
#    target sym/tf — backtest reads its history).

# 2. Set DB connection.
export DATABASE_URL='postgres://user:pass@localhost/qtss'

# 3. Run a single backtest.
cargo run --release -p qtss-backtest --bin iq-backtest -- \
    --config crates/qtss-backtest/examples/btc_4h_dip.json \
    --log /tmp/btc_4h_trades.jsonl
```

The CLI prints a summary like:

```
─── IQ Backtest Report ───────────────────────────────
 run_tag: btc_4h_dip_baseline
 universe: binance/futures/BTCUSDT 4h
 window:   2025-01-01T00:00:00Z -> 2026-04-01T00:00:00Z
 bars:     2200

 trades:   47
   wins:     22 (47.8%)
   losses:   24
   scratches: 1

 gross_pnl:    1250.00
 net_pnl:      1080.00
 max_dd:       8.5%
 profit_factor: 1.65
 sharpe(per-trade): 0.42

 loss reasons:
                    StopLossNoTp: 14
       StopLossAfterPartialTp:  6
            TimeoutNoProgress:  3
            TrailingStopAfterMfe:  1

 avg component scores on losers:
      fib_retrace_quality: 0.31  ← weakest channel
              volume_capit: 0.42
       wyckoff_alignment: 0.62
──────────────────────────────────────────────────────
```

## Trade log format

Each line of the JSONL is one closed (or open-at-end) trade with
its full attribution:

```json
{
  "trade": {
    "trade_id": "uuid",
    "entry_bar": 1855,
    "entry_time": "2026-04-02T12:00:00Z",
    "entry_price": "65676.10",
    "polarity": "dip",
    "entry_components": {
      "structural_completion": 0.7,
      "fib_retrace_quality":   0.6,
      "volume_capitulation":   0.85,
      "wyckoff_alignment":     1.0,
      "cycle_alignment":       1.0,
      ...
    },
    "exit_bar": 1976,
    "exit_price": "79444.00",
    "outcome": "take_profit_full",
    "net_pnl_pct": 12.3,
    "max_favorable_pct": 14.1,
    "max_adverse_pct":   -1.2,
    "tier_pnls": ["100", "100", "200", "0"]
  },
  "attribution": {
    "class": "win",
    "loss_reason": null,
    "path": {
      "max_favorable_pct": 14.1,
      "bars_held": 121,
      "bars_to_first_tp": 8,
      "realised_rr": 11.75
    },
    "components": {
      "weakest_channel": "fib_retrace_quality",
      "weakest_score": 0.6,
      "strongest_channel": "wyckoff_alignment"
    }
  }
}
```

Slice + dice with DuckDB / pandas:

```sql
-- DuckDB on the JSONL
SELECT
  attribution->>'class'           AS class,
  attribution->>'loss_reason'     AS loss_reason,
  COUNT(*)                        AS n,
  AVG(CAST(trade->>'net_pnl_pct' AS DOUBLE)) AS avg_pnl_pct
FROM read_json_auto('/tmp/btc_4h_trades.jsonl')
GROUP BY 1, 2
ORDER BY n DESC;
```

## Loss attribution categories

Every losing trade is classified into ONE of these `LossReason`s:

| Reason                     | Trigger                                         |
|----------------------------|-------------------------------------------------|
| `StopLossNoTp`             | SL hit before any TP                            |
| `StopLossAfterPartialTp`   | SL hit after TP1/TP2 — salvageable but worse    |
| `TrailingStopAfterMfe`     | Trail dragged in profit then stopped out        |
| `TimeoutNoProgress`        | Hit max_holding_bars near flat (≥ -0.5%)        |
| `TimeoutNegative`          | Hit max_holding_bars deep red (< -0.5%)         |
| `InvalidationEvent`        | External flip (Wyckoff event opposite direction)|
| `MfeBeyondSlButNoTp`       | Was profitable but never hit a TP, then SL      |
| `CostsOnly`                | Closed @ TP but cost > realised price move      |

The aggregate report's `loss_reason_counts` histogram tells you
which category dominates — that's where the biggest fix lives.
E.g. lots of `MfeBeyondSlButNoTp` → TPs are too aggressive; lots
of `TimeoutNoProgress` → entry threshold is too lax / market is
choppy in this regime.

## Component-weakness on losers

The report's `avg_loss_components` map gives the AVERAGE entry
score for each channel ACROSS all losing trades. If
`fib_retrace_quality` averages 0.31 on losers (vs 0.6 baseline on
all trades), the entry threshold isn't filtering for fib quality
strictly enough — bump that channel's weight or add a per-channel
floor.

## Walk-forward optimisation

```rust
use qtss_backtest::iq::{
    IqBacktestConfig, GridSpec, WeightRange, WalkForwardSpec,
    OptimizationRunner,
};
use chrono::{Duration, Utc, TimeZone};

let base = IqBacktestConfig::default();
let grid = GridSpec {
    wyckoff_alignment: Some(WeightRange { min: 0.10, max: 0.20, step: 0.02 }),
    cycle_alignment:   Some(WeightRange { min: 0.05, max: 0.15, step: 0.02 }),
    normalise_to: Some(1.0),
    ..Default::default()
};
let wf = WalkForwardSpec {
    in_sample: Duration::days(120),
    out_of_sample: Duration::days(30),
    slide_step: Duration::days(30),
    start_at: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
    end_at: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
};
let runner = OptimizationRunner::new(base, grid, wf);
let report = runner.run(&pool).await?;
```

`OptimizationReport` carries:
  - `leaderboard`: every weight config ranked by mean OOS score
  - `sensitivity`: Pearson r between each channel weight and OOS
    score across all configs (high |r| = channel matters)

## Live parity guarantee

Every score function in `scorers.rs` mirrors a `score_*` function
in `crates/qtss-worker/src/major_dip_candidate_loop.rs` — same
table, same column, same threshold logic. The ONLY difference is
the `WHERE ... <= bar_time` cutoff in backtest queries. Running a
backtest over bars the live worker has already processed produces
identical channel scores for each bar.

Implication: weight knobs you tune in backtest will produce the
same signal shape live.

## Status (FAZ 26 release)

| Component                    | Status |
|------------------------------|--------|
| Bar replay (IqBacktestRunner)| ✓      |
| 10-channel composite scoring | ✓      |
| TP ladder + SL + trailing + timeout | ✓ |
| Cost model (fee+slip+funding) | ✓     |
| Loss attribution (8 reasons)  | ✓     |
| JSONL trade log               | ✓     |
| Aggregate report              | ✓     |
| Grid search                   | ✓     |
| Walk-forward                  | ✓     |
| Sensitivity analysis          | ✓     |
| CLI binary                    | ✓     |
| Bayesian optimisation         | parked |
| Parallel execution            | parked |
| GUI Backtest Studio           | parked |

Next wave (the chart-audit backlog) lives at
`docs/WYCKOFF_CHART_BACKLOG.md`.
