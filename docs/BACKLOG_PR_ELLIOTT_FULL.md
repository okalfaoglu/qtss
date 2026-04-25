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
