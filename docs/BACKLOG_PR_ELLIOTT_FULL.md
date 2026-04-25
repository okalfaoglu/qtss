# Backlog — kept open while PR-ELLIOTT-FULL ships

Created 2026-04-26. User instruction: "bitene kadar yeni bug gelirse backloga at."

## 1. SOURCE=live slot toggle stickiness
Repro: `/v2/iq-chart` ETHUSDT 4h source=`live`.
- Z3 alone works.
- Z3 unchecked → Z4 disappears too.
- Z4 unchecked → Z5 disappears too.
- Same chain for Z1 → Z2 → … with Z2/Z4.
Hypothesis: `enabledLengths.join(",")` collapses to a string the
`/v2/elliott` endpoint can't satisfy when the ZigZag length set is
non-contiguous; Pine port likely groups computation per slot but
falls back to the highest-active when one is missing.

## 2. ETH 4h post-motive (5) corrective never emits ABC family
DB query (mode=live, slot=3, family∈abc/abc_*) returns ZERO rows for
the Z4 motive ending 2026-04-14 even though chart visibly has clean
post-(5) pivots. User suspicion confirmed.
Suspected cause: `elliott_early.rs::write_early` ABC scanner drops
candidates when `first_valid` direction filter never finds an
opposite-direction pivot among the post-W5 sample, or the mini-pivot
fallback (window=3) misses them on this geometry. Need a focused trace
on ETH 4h Z4 to see exactly which guard is short-circuiting.

## 3. iq_d entry_price bug (deferred from earlier session)
W3 entry tier sets `entry_price = anchor[W3].price` (the W3 PEAK).
For trend-following continuation the entry should fire at W3 BREAKOUT
mid-leg, not at the closing peak. ETHUSDT 1w showed long armed at
$4098 against $2300 spot before the staleness gate caught it.

✅ Resolved in commit `5bf21bf` (W3 → W1 + buffer breakout entry).

## 4. Trade-open TP-proximity gate + actual fill price display
Triggered by user after FAZ 25.2.D completes.

Symptom (BTCUSDT 15m IQ-T setup `c524ef70`):
- GİRİŞ (setup entry): $77168.43
- ANLIK (current):    $77536.25
- TP:                  $77599.90 (only +0.56% from setup entry)
- Distance left to TP: $63 / total target distance $432 = 14% remaining
- Position armed shows +0.48% in profit but trade has 86% of move
  already gone — almost no upside left, big asymmetry vs SL distance.

Two asks:

(a) **Pre-fill TP-proximity gate** in `execution_bridge.rs::
build_live_position_for_mode` — before inserting the position,
compute:
```
remaining_to_tp_pct = |tp - current_price| / |tp - entry_price|
```
If `remaining_to_tp_pct < cfg.min_tp_remaining_fraction` (default 0.5,
operator-tunable), reject the trade. The setup is already too close
to its target to be worth opening — TP would fill almost immediately
on a benign tick, distorting reports.

(b) **Show actual fill price separately from setup entry**. The GUI
currently labels `live_positions.entry_avg` as "GİRİŞ" but the user
reads "GİRİŞ" as "the price the setup said to enter at" — which is
`qtss_setups.entry_price`. Two distinct numbers for two distinct
events:
- Setup entry (target buy/sell level the system armed): from
  `qtss_setups.entry_price`
- Actual fill (paper price after slippage / market price for live):
  from `live_positions.entry_avg`
Frontend (`/v2/reports` setup detail panel) needs both fields with
separate labels: e.g. "Hedef Giriş" vs "Açılış Fiyatı".
