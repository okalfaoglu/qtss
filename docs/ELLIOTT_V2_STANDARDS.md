# Elliott V2 Standards

This document defines naming, coloring, wave labels, hard rules, and ZigZag scope for the Elliott V2 engine.

## 1) Timeframe Hierarchy

- Macro: `4h`
- Intermediate: `1h`
- Micro: `15m`

The chart anchor is `15m`. `1h` and `4h` are rendered as overlays on the same chart.

## 2) Naming and Labeling

- `4h` labels:
  - Impulse: `1 2 3 4 5`
  - Corrective: `A B C`
- `1h` labels:
  - Impulse: `(1) (2) (3) (4) (5)`
  - Corrective: `(A) (B) (C)`
- `15m` labels:
  - Impulse: `i ii iii iv v`
  - Corrective: `a b c`

## 3) Color Standard

- Macro `4h`: `#1E88E5` (blue)
- Intermediate `1h`: `#43A047` (green)
- Micro `15m`: `#FB8C00` (orange)
- Invalidation markers: `#E53935` (red)
- Neutral/assistive hints: `#B0BEC5`

## 4) Elliott Rule Scope (V2.0)

### Hard invalidation rules for impulse

1. Wave 2 cannot retrace beyond the start of Wave 1.
2. Wave 3 cannot be the shortest among Waves 1, 3, 5.
3. Wave 4 cannot overlap Wave 1 price territory (diagonal exception is not enabled in V2.0).

### Corrective scope

- Enabled: `ABC`, `zigzag (5-3-5)`, `flat (3-3-5)`.
- Deferred to later versions: full triangle grammar, W-X-Y combinations, advanced diagonal handling.

## 5) ZigZag Policy

- Elliott decision engine uses one ZigZag logic per timeframe.
- Channel module can keep multiple ZigZag configs for drawing/scanning.
- Channel ZigZag output must not directly invalidate Elliott state.

## 6) State Model

- Parent-child chain: `4h -> 1h -> 15m`.
- If micro count invalidates:
  - reset only micro state,
  - keep parent states unless parent hard rules fail.

## 7) Confidence and Decisions

- Decision class in V2.0: `invalid`, `candidate`, `confirmed`.
- Confidence should be derived from:
  - hard rules pass/fail,
  - structure completeness,
  - ratio proximity (soft scoring).
