import type { DiagonalRoleV2, ImpulseCountV2, TimeframeStateV2 } from "./types";

/** Minimum fraction of micro diagonal time span that must fall inside parent W1 or W5. */
const MIN_COVERAGE = 0.22;
/** Minimum excess of one overlap over the other to pick a side. */
const MARGIN = 0.12;

/**
 * Epoch span of the diagonal window (p0–p5), ordered low→high.
 */
export function microDiagonalSpanEpochSeconds(imp: ImpulseCountV2): [number, number] {
  const t0 = imp.pivots[0].time;
  const t5 = imp.pivots[5].time;
  return [Math.min(t0, t5), Math.max(t0, t5)];
}

/**
 * Parent impulse wave-1 and wave-5 time spans from pivot times.
 */
export function parentWave1Wave5EpochSpans(parent: ImpulseCountV2): { w1: [number, number]; w5: [number, number] } {
  const p = parent.pivots;
  const w1: [number, number] = [Math.min(p[0].time, p[1].time), Math.max(p[0].time, p[1].time)];
  const w5: [number, number] = [Math.min(p[4].time, p[5].time), Math.max(p[4].time, p[5].time)];
  return { w1, w5 };
}

function spanOverlapFraction(micro: [number, number], wave: [number, number]): number {
  const ml = micro[0];
  const mh = micro[1];
  const wl = Math.min(wave[0], wave[1]);
  const wh = Math.max(wave[0], wave[1]);
  const lo = Math.max(ml, wl);
  const hi = Math.min(mh, wh);
  const ov = Math.max(0, hi - lo);
  const len = mh - ml;
  return len > 1e-9 ? ov / len : 0;
}

/**
 * Compare micro diagonal time span to parent W1 vs W5 spans (v1: motive placement only).
 * Returns null when overlap does not favor one side clearly.
 */
export function inferDiagonalRoleFromParentImpulse(
  micro: ImpulseCountV2,
  parent: ImpulseCountV2,
  parentTfLabel: string,
): { role: DiagonalRoleV2; detail: string } | null {
  const microSpan = microDiagonalSpanEpochSeconds(micro);
  const { w1, w5 } = parentWave1Wave5EpochSpans(parent);
  const c1 = spanOverlapFraction(microSpan, w1);
  const c5 = spanOverlapFraction(microSpan, w5);
  const leadingOk = c1 >= MIN_COVERAGE && c1 >= c5 + MARGIN;
  const endingOk = c5 >= MIN_COVERAGE && c5 >= c1 + MARGIN;
  if (leadingOk && !endingOk) {
    return {
      role: "leading",
      detail: `MTF ${parentTfLabel}: micro span vs parent W1 ${(c1 * 100).toFixed(0)}% (W5 ${(c5 * 100).toFixed(0)}%)`,
    };
  }
  if (endingOk && !leadingOk) {
    return {
      role: "ending",
      detail: `MTF ${parentTfLabel}: micro span vs parent W5 ${(c5 * 100).toFixed(0)}% (W1 ${(c1 * 100).toFixed(0)}%)`,
    };
  }
  return null;
}

export function isParentStateUsableForMtfDiagonal(state: TimeframeStateV2 | null | undefined): boolean {
  if (!state?.impulse) return false;
  if (state.decision === "invalid") return false;
  return true;
}

/**
 * `15m` diagonal: try `1h` parent first, then `4h`.
 */
export function pickMtfDiagonalRoleForMicroTf(
  micro: ImpulseCountV2,
  s1h: TimeframeStateV2 | null | undefined,
  s4h: TimeframeStateV2 | null | undefined,
): { role: DiagonalRoleV2; detail: string } | null {
  if (isParentStateUsableForMtfDiagonal(s1h) && s1h?.impulse) {
    const r = inferDiagonalRoleFromParentImpulse(micro, s1h.impulse, "1h");
    if (r) return r;
  }
  if (isParentStateUsableForMtfDiagonal(s4h) && s4h?.impulse) {
    const r = inferDiagonalRoleFromParentImpulse(micro, s4h.impulse, "4h");
    if (r) return r;
  }
  return null;
}

/**
 * `1h` diagonal: use `4h` parent only.
 */
export function pickMtfDiagonalRoleFor1hTf(
  micro: ImpulseCountV2,
  s4h: TimeframeStateV2 | null | undefined,
): { role: DiagonalRoleV2; detail: string } | null {
  if (isParentStateUsableForMtfDiagonal(s4h) && s4h?.impulse) {
    return inferDiagonalRoleFromParentImpulse(micro, s4h.impulse, "4h");
  }
  return null;
}
