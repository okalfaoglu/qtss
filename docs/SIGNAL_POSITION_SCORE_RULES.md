# Position strength (0–10) — trend and scenario rules

Canonical reference for QTSS signal UX and automation hints. **Code:** `qtss_chart_patterns::score_trend` (`roll_position_strength_history`, `classify_score_trend`, `classify_position_scenario`). **Payload:** `signal_dashboard` JSON after confluence merge — `position_strength_history_10`, `score_trend_kind`, `score_trend_action`, optional `position_strength_entry_10`, `position_scenario_kind`.

## Score trend (last three samples, oldest → newest)

Rolling window is persisted on each snapshot; classification runs only when **three** samples exist.

| Pattern (examples) | Meaning (TR) | `score_trend_kind` | `score_trend_action` |
|--------------------|--------------|-------------------|----------------------|
| 6 → 7 → 8 ▲ | İyileşiyor | `improving` | `ease_toward_tp` |
| 7 → 7 → 7 — | Sabit | `stable` | `watch_no_issue` |
| 8 → 6 → 5 ▼ | Kötüleşiyor | `worsening` | `tighten_stop` |
| 6 → 4 → 3 ▼ | Hızlı düşüş | `rapid_decline` | `plan_exit_or_wait_sl` |
| 5 → 3 → 2 ▼ | Serbest düşüş | `free_fall` | `act_immediately` |

Implementation uses these examples plus monotonic and delta heuristics; see `score_trend.rs` tests.

## Entry vs current scenario (LONG/SHORT active only)

`position_strength_entry_10` is set when status becomes LONG/SHORT from NOTR (or first observation) and carried until status returns to NOTR.

| Scenario (TR) | Condition (approx.) | `position_scenario_kind` |
|---------------|---------------------|----------------------------|
| Mükemmel, pozisyon güçleniyor | `current > entry` and `current >= 8` | `strengthening_excellent` |
| İyi, stabil | `current == entry` and `entry >= 8` | `stable_good` |
| Tehlike, trend dönüyor | `entry >= 7` and `current <= 5` and falling | `danger_reversal` |
| Dikkat, momentum bitiyor | `entry >= 9` and `current <= 6` and falling | `momentum_fading` |
| (no label) | else | `none` |

## Band reference (position quality)

For copy/UI copy only; thresholds may be enforced elsewhere.

| Band | TR note |
|------|---------|
| 10–7 | Güçlü |
| 6 | Orta |
| 5–4 | Zayıf |
| ≤3 | Kritik — pozisyondan uzak dur |

## Position protection (concept)

Extra buffer under stop vs wicks/noise — operational detail for execution/risk modules; not computed in `score_trend`.

## Risk-off (concept)

Machine reduces or pauses T-analysis output; only high-strength structures — policy layer, not `score_trend`.
