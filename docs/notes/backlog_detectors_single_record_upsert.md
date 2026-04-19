# Backlog — All detectors must use single-record upsert

**Context:** `qtss_v2_detections` had 36.67M rows, of which 36.02M were
`family='elliott'`. Root cause: Elliott (and by inference every other
detector except TBM as of commit `c2e8c78`) re-emits the same logical
pattern every tick. Each re-emission:

1. Inserts a brand-new row (new UUID) with `state='forming'`.
2. The invalidation sweeper then flips older rows to `state='invalidated'`.
3. Terminal invalidated rows accumulate forever.

Example: BTCUSDT 15m `leading_diagonal_5_3_5_bull` anchor
`2020-08-25T18:00:00Z` had **14099 invalidated duplicate rows** for a
single logical pattern.

Migration 0184 performs one-shot purge + adds a partial UNIQUE index
on open states `(exchange, symbol, timeframe, family, subkind,
anchors->0->>'time')` → future duplicate inserts on a live key will
fail at DB level. **But** writer code still tries to INSERT on every
tick, which will now raise `unique_violation`. We need to migrate
every detector to the upsert pattern TBM already uses
(commit c2e8c78).

## Pattern (from v2_tbm_detector.rs)

```rust
let open_rows = repo
    .list_open_by_key(exchange, symbol, timeframe, family, subkind)
    .await?;

if let Some((primary, duplicates)) = open_rows.split_first() {
    for dup in duplicates {
        repo.update_state(dup.id, "invalidated").await?;
    }
    let locked = matches!(primary.state.as_str(), "confirmed" | "entry_ready");
    let merged_meta = merge_raw_meta(&primary.raw_meta, &new_meta);
    let next_anchors = if locked { primary.anchors.clone() } else { new_anchors };
    let next_inv = if locked { primary.invalidation_price } else { new_inv };
    repo.update_anchor_projection(primary.id, score, next_inv, next_anchors, merged_meta).await?;
} else {
    repo.insert(NewDetection { .. }).await?;
}
```

## Affected detectors

- [ ] **elliott** — biggest offender (35.5M rows). Files:
  - `crates/qtss-worker/src/v2_elliott_detector.rs` — main loop
  - `crates/qtss-worker/src/v2_elliott_*.rs` — variants
- [ ] **range** — 369K invalidated rows
  - `crates/qtss-worker/src/v2_range_detector.rs`
- [ ] **harmonic** — 38K invalidated rows
  - `crates/qtss-worker/src/v2_harmonic_detector.rs`
- [ ] **classical** — 23K invalidated rows
  - `crates/qtss-worker/src/v2_classical_detector.rs`
- [ ] **candle** — 1.3K rows (minor)
  - `crates/qtss-worker/src/v2_candle_detector.rs`

Wyckoff uses the legacy `wyckoff_structures` table, not `qtss_v2_detections`
(confirmed rows come via a separate `v2_detection_orchestrator` path).
Single-record semantics there are governed by migration 0182 UNIQUE
index + record_event_with_time `(event, time_ms)` dedup.

## Key per detector

The natural key for the partial UNIQUE index added in 0184 is:
```
(exchange, symbol, timeframe, family, subkind, anchors->0->>'time')
```

Detectors that anchor on multiple bars (elliott labels waves 1/3/5)
store ALL anchors in `anchors` as an array. The first element is
always the earliest pivot, so keying on `anchors->0` gives stable
identity.

## Acceptance

1. After the migration, live detector ticks don't raise
   `unique_violation` errors in logs.
2. Row count growth of `qtss_v2_detections` is bounded — only new
   anchor appearances add rows, refreshes update in place.
3. Chart overlays don't show duplicate labels for the same anchor bar.

## Next steps

1. Extract the TBM upsert helper into a shared utility
   (`crates/qtss-worker/src/detection_upsert.rs`).
2. Port each detector file-by-file; run integration test
   `backtest_regression` after each port to confirm no row-count
   blowup.
3. Close this backlog when all five detectors upsert.
