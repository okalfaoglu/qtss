# bug: TBM chart overlay is missing Entry / TP lines

**Reported:** 2026-04-19
**Area:** web-v2 Chart (TBM panel) + backend render_geometry
**Severity:** medium — overlay is partially drawn so users see only SL and
a plain `bottom_setup 44% · Weak` / `top_setup 38% · Weak` label. Entry
and TP1/TP2/TP3 price levels are not rendered, so the operator cannot
read the setup at a glance.

## Symptom

BTCUSDT · binance · futures · 1h, TBM panel enabled with the "Entry /
TP / SL" toggle ON and "Aa Labels" ON.

What should render (same as Wyckoff overlay):

- `Entry`  price line (blue-ish)
- `TP1`, `TP2`, `TP3` price lines (green)
- `SL` price line (red) — **this one renders**

What actually renders: only the `SL 74076.06` line on the right axis
and the two colored square labels at the anchor bar
(`top_setup 38% · Weak`, `bottom_setup 44% · Weak`).

## Where to look

1. Detection row: `qtss_v2_detections` where `family='tbm'`. Check
   `render_geometry` / `render_labels` / `raw_meta` — is the TP/Entry
   block being persisted? Pattern to follow: Wyckoff detector persists
   TP1/TP2/TP3 inside `raw_meta` via `trade_planner.rs`.
2. TBM confirm path: `crates/qtss-worker/src/v2_tbm_detector.rs`
   `confirm_forming_or_timeout` — when the row flips to `confirmed`,
   does it plan an entry/TP ladder? Currently it stores only
   `bos_bar_index`, `followthrough_bar_index`, `wyckoff_events`. No
   TP ladder is computed for TBM — the SL visible on the chart is
   likely derived from `invalidation_price` alone.
3. Chart renderer: `web-v2/src/pages/Chart.tsx` — locate the TBM
   overlay dispatch. Verify it reads the same Entry/TP fields the
   Wyckoff dispatcher reads, or that a TBM-specific dispatch exists.

## Root-cause hypothesis

TBM detector never plans a trade — it only marks structural reversal
candidates. The Entry/TP/SL ladder is the setup-engine's job
(`qtss-setup-engine::PositionGuard` via `v2_setup_loop`). So either:

a. The setup-engine hasn't armed a setup for this TBM row yet
   (row is still `forming` or just `confirmed` without a setup), but
   the chart's TBM overlay is trying to display Entry/TP anyway and
   falling back to only SL. **Fix:** overlay should render Entry/TP
   only when an armed `qtss_setups` row exists for this detection.
b. The setup-engine did arm a setup but the TBM overlay reads from
   `qtss_v2_detections.raw_meta` instead of joining `qtss_setups`.
   **Fix:** extend the chart API payload to include
   entry_price / tp1 / tp2 / tp3 / sl from `qtss_setups` when a
   matching `detection_id` exists.

## Fix plan (sketch)

1. Check `GET /v2/chart/overlays` (or whichever endpoint powers the
   TBM overlay) payload for a BTCUSDT 1h confirmed TBM row — confirm
   whether entry/TP fields are present.
2. If absent: JOIN `qtss_setups` on `detection_id` and project
   `entry_price`, `tp1_price`, `tp2_price`, `tp3_price`, `sl_price`.
3. Update the Chart.tsx TBM dispatch to render `createPriceLine` for
   each of the four levels with matching colors + the
   `Entry / TP / SL` toggle already in the UI.
4. If no armed setup exists for a confirmed TBM row, gracefully hide
   the Entry/TP lines (leave the SL proxy from invalidation_price).

## Acceptance

- A confirmed TBM row with an armed setup renders all four lines
  (Entry, TP1, TP2, TP3) + SL on the chart.
- A confirmed TBM row without a setup renders only the SL proxy and
  no ghost Entry/TP lines.
- Toggle "Entry / TP / SL" OFF hides every line.
