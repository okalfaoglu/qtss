# Wyckoff Method — QTSS Implementation Reference

This document is the canonical reference for QTSS's Wyckoff event
detectors. Each event maps to a specific phase of the Wyckoff
Accumulation or Distribution schematic; the calculation method
behind each detector is documented here side-by-side with the
classical doctrine so the engine output stays auditable against
the reference sources.

## References

- StockCharts ChartSchool — Wyckoff Method tutorial:
  <https://chartschool.stockcharts.com/table-of-contents/market-analysis/wyckoff-analysis-articles/the-wyckoff-method-a-tutorial>
- Wyckoff Analytics — Method overview:
  <https://www.wyckoffanalytics.com/wyckoff-method/>
- Trading Wyckoff — Phase A/B/C/D/E walkthrough:
  <https://tradingwyckoff.com/en/phases-of-a-wyckoff-structure/>
- TradingView — Reference chart with annotated phases:
  <https://tr.tradingview.com/chart/XU100/v5Shj6tF/>

## Phase Map

The two schematics that frame everything below.

```
ACCUMULATION                       DISTRIBUTION
───────────────────────────────    ───────────────────────────────
Phase A (Stopping action)          Phase A (Stopping action)
  PS  Preliminary Support            PSY Preliminary Supply
  SC  Selling Climax                 BC  Buying Climax
  AR  Automatic Rally                AR  Automatic Reaction
  ST  Secondary Test                 ST  Secondary Test

Phase B (Building cause)           Phase B (Building cause)
  ST  Secondary Tests (multiple)     ST  Secondary Tests (multiple)
                                     UT  Upthrust (minor)

Phase C (Test of supply / demand)  Phase C (Test of demand / supply)
  Spring (or Shakeout)               UTAD Upthrust After Distribution
  Test of Spring                     Test of UTAD

Phase D (Move out of TR)           Phase D (Move out of TR)
  SOS  Sign of Strength              SOW  Sign of Weakness
  LPS  Last Point of Support         LPSY Last Point of Supply
  BU   Backup / Jump-Across-Creek    BO   Breakdown
  
Phase E (Markup)                   Phase E (Markdown)
```

## Per-Event Calculation Methods

All detectors take `(bars: &[Bar], cfg: &WyckoffConfig)` and emit
`Vec<WyckoffEvent>`. The shared bar helpers (`bar_high`, `bar_low`,
`bar_range`, `bar_vol`, `sma_volume`, `atr`) are defined at the top
of `events.rs`.

### Accumulation

#### PS — Preliminary Support
- **Doctrine:** First sign that the relentless decline is starting
  to attract buyers, marked by an increase in volume and a wider
  spread.
- **QTSS rule (`eval_ps`):** Bar's range >= `climax_range_atr_mult`
  × ATR(14) and volume >= `climax_volume_mult` × SMA(volume, 20)
  inside a sustained downtrend (last 30 bars made a fresh low).
  Anchor: `bar_low`.
- **Phase:** A.

#### SC — Selling Climax
- **Doctrine:** Wide-range, ultra-high-volume capitulation flush
  with a long lower wick — panic selling absorbed by smart money.
- **QTSS rule (`eval_selling_climax`):** Bar at trend-bottom
  (gate: `at_trend_bottom`, fresh 30-bar low + 1% breakdown
  buffer), volume >= `climax_volume_mult` × SMA, range >=
  `climax_range_atr_mult` × ATR, body <= 20% of range, lower wick
  > 40% of range.
  Anchor: `bar_low`.
- **Phase:** A.

#### AR — Automatic Rally
- **Doctrine:** First reaction up after the SC. Defines the upper
  bound of the trading range.
- **QTSS rule (`eval_ar`):** Within 8 bars of an SC, scan for the
  highest close that is ≥ `ar_min_rally_pct` above the SC low.
  Anchor: peak bar's `bar_high`.
- **Phase:** A.

#### ST — Secondary Test
- **Doctrine:** Low-volume retest of the SC area; supply has
  weakened.
- **QTSS rule (`eval_st`):** Bar low touches within
  `st_proximity_pct` of the SC low and bar volume <= prior SMA(20)
  volume (lighter than the SC).
  Anchor: `bar_low`.
- **Phase:** A / B.

#### Spring (Shakeout)
- **Doctrine:** Phase C manipulation. Price punches BELOW the TR
  low, fails, and reclaims the range — trapping shorts and
  defining a higher low.
- **QTSS rule (`eval_spring`):** Bar low pierces the
  `spring_lookback`-bar range low by >= `spring_min_pierce_pct`
  AND closes back above the range low. Volume gate (BUG fix
  2026-04-26): volume_ratio in [`spring_min_volume_mult`,
  `spring_max_volume_mult`] — Wyckoff doctrine demands the Spring
  be ABSORBED, not screamed (a too-high volume spike is a
  capitulation continuation, not a Spring).
  Anchor: `bar_low`.
- **Phase:** C.

#### Test (of Spring)
- **Doctrine:** Lower-volume retest of the Spring low confirming
  supply exhaustion.
- **QTSS rule (`eval_test`):** Within
  `test_lookforward_bars` after a Spring, scan for a bar whose
  low touches within `test_proximity_pct` of the Spring low AND
  volume < prior 20-bar SMA volume.
  Anchor: `bar_low`.
- **Phase:** C.

#### SOS — Sign of Strength
- **Doctrine:** First wide-range bull bar with strong volume,
  signalling demand has overpowered supply.
- **QTSS rule (`eval_sos`):** body > 0 AND body > 60% of range AND
  range >= `sos_amplifier` × ATR AND volume >= `sos_amplifier` ×
  SMA(20). The amplifier doubles as both range and volume gate so
  the detector only fires on bars that visually stand out on the
  tape.
  Anchor: `bar_close`.
- **Phase:** D.

#### LPS — Last Point of Support
- **Doctrine:** Higher-low pullback after the SOS that confirms
  buyers will now defend the higher level.
- **QTSS rule (`eval_lps`):** Within
  `lps_lookforward_bars` after an SOS, scan for a bar whose low
  prints HIGHER than the prior pullback low AND volume <= prior
  SMA(20) volume (light pullback).
  Anchor: `bar_low`.
- **Phase:** D.

#### BU/JAC — Backup / Jump-Across-the-Creek
- **Doctrine:** Price breaks the "creek" (range top) and then
  drops back to retest it from above on lighter volume.
- **QTSS rule (`eval_bu`):** SOS in the recent past →
  define creek as the 20-bar high before SOS → scan
  `lps_lookforward_bars` ahead for a bar whose low touches the
  creek within 0.5% AND closes back ABOVE the creek AND volume
  is light (< SMA(20)).
  Anchor: bar's `bar_low` (the retest pierce).
- **Phase:** D.

### Distribution

#### PSY — Preliminary Supply
- **Doctrine:** Mirror of PS. First wide-range, volume-spike bar
  AT the top of an uptrend that hints at distribution starting.
- **QTSS rule (`eval_ps_distribution`):** Same shape as PS but in
  uptrend context: 30-bar high + 1% breakout buffer, volume +
  range gates as PS, and bar must trend UP into the high.
  Anchor: `bar_high`.
- **Phase:** A.

#### BC — Buying Climax
- **Doctrine:** Mirror of SC. Wide-range, ultra-high-volume blow-
  off top with long upper rejection wick.
- **QTSS rule (`eval_buying_climax`):** Trend-top gate
  (`at_trend_top`, fresh 30-bar high + 1% breakout buffer),
  volume >= `climax_volume_mult` × SMA, range >=
  `climax_range_atr_mult` × ATR, weak body (>= -20% of range,
  i.e. allow small bear bodies), upper wick > 40% of range.
  Anchor: `bar_high`.
- **Phase:** A.

#### AR (Distribution) — Automatic Reaction
- **Doctrine:** First selloff after BC. Defines the lower bound
  of the distribution range.
- **QTSS rule (`eval_ar_distribution`):** Within 8 bars of a BC,
  scan for the lowest close that is >= `ar_min_drop_pct` BELOW
  the BC high.
  Anchor: trough bar's `bar_low`.
- **Phase:** A.

#### ST (Distribution) — Secondary Test
- **Doctrine:** Low-volume retest of the BC area.
- **QTSS rule (`eval_st_distribution`):** Bar high touches within
  `st_proximity_pct` of BC high, volume < prior SMA(20).
  Anchor: `bar_high`.
- **Phase:** A / B.

#### UTAD — Upthrust After Distribution
- **Doctrine:** Phase C manipulation top. Price punches ABOVE the
  TR high, fails, and falls back into the range — trapping longs.
- **QTSS rule (`eval_utad`):** Bar high pierces the
  `spring_lookback`-bar range high by >= `spring_min_pierce_pct`
  AND closes back below the range high. Same volume gate as
  Spring (BUG fix 2026-04-26): volume_ratio in
  [`spring_min_volume_mult`, `spring_max_volume_mult`].
  Anchor: `bar_high`.
- **Phase:** C.

#### Test (of UTAD) — `test_distribution`
- **Doctrine:** Lower-volume retest of the UTAD high confirming
  demand exhaustion.
- **QTSS rule (`eval_test_distribution`):** Within
  `test_lookforward_bars` after a UTAD, scan for a bar whose high
  touches within `test_proximity_pct` of the UTAD high AND
  volume < prior SMA(20) volume.
  Anchor: `bar_high`.
- **Phase:** C.

#### SOW — Sign of Weakness
- **Doctrine:** First wide-range bear bar with strong volume,
  signalling supply has overpowered demand.
- **QTSS rule (`eval_sow`):** body < 0 AND |body| > 60% of range
  AND range >= `sos_amplifier` × ATR AND volume >= `sos_amplifier`
  × SMA(20).
  Anchor: `bar_close`.
- **Phase:** D.

#### LPSY — Last Point of Supply
- **Doctrine:** Lower-high bounce after SOW confirming sellers
  will defend the lower level.
- **QTSS rule (`eval_lps_distribution`):** Within
  `lps_lookforward_bars` after a SOW, scan for a bar whose high
  prints LOWER than the prior bounce high AND volume <= SMA(20).
  Anchor: `bar_high`.
- **Phase:** D.

#### BO/Breakdown — `bu_distribution`
- **Doctrine:** Mirror of BU. Price breaks the "ice" (range
  bottom) then bounces back to retest it from below on lighter
  volume.
- **QTSS rule (`eval_bu_distribution`):** SOW in the recent past
  → define ice as the 20-bar low before SOW → scan
  `lps_lookforward_bars` ahead for a bar whose high touches the
  ice within 0.5% AND closes back BELOW the ice AND volume is
  light (< SMA(20)).
  Anchor: bar's `bar_high` (the retest pierce).
- **Phase:** D.

## Persistence

Every detected event is written to `detections` with:
- `pattern_family = 'wyckoff'`
- `subkind` = lowercase event name with bull/bear suffix
  (e.g. `spring_bull`, `utad_bear`, `sos_bull`)
- `direction` = +1 (bullish) / -1 (bearish)
- `start_time`, `end_time` = bar's open_time
- `slot` = detector slot (Z0..Z5)
- `mode = 'live'`
- `raw_meta` = `{ score, volume_ratio, range_ratio, note }`
- `anchors` = single-element JSONB carrying the event bar's
  reference price + bar_index

## Live use

The event evaluators run inside `qtss-engine::writers::wyckoff` on
every worker tick (typically once per minute). Confirmed events
are upserted into `detections` with `ON CONFLICT(start_time,
end_time, subkind) DO UPDATE` so re-runs over the same bar are
idempotent.

The cycle pipeline in `cycle.rs` consumes confirmed events plus
Elliott segments to build the four-phase rotation tile timeline
(`cycle_accumulation` / `cycle_markup` / `cycle_distribution` /
`cycle_markdown`) used by the GUI overlay.

## Chart anchoring

Each event renders at its `reference_price` y-coordinate at the
bar's `open_time` x-coordinate. SC / Spring / PS / LPS use the
bar's LOW (the actionable level for buyers). BC / UTAD / PSY /
LPSY / SOS-top variants use the bar's HIGH. SOS / SOW use
`bar_close` (where the strength / weakness was confirmed). AR
uses the rally-peak / reaction-trough anchor.

## Cycle phase tags on labels

Chart labels carry `· PA / PB / PC / PD / PE` suffixes derived
from the surrounding cycle tile so the operator sees the schematic
phase at a glance:

- `Spring · PC`  → Spring fired during a Phase-C tile.
- `BC · PA`     → Buying Climax fired during the Phase-A stopping action.
- `Markdown · Z1 ◆` → cycle tile, source Confluent (Elliott + Event
   agreement) at slot 1.
