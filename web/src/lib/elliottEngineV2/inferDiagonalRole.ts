import type { DiagonalRoleV2, ImpulseCountV2, ZigzagPivot } from "./types";

/** Early pivot start + late pivot end → chart-window hint for leading vs ending diagonal. */
export function inferDiagonalRoleFromChart(imp: ImpulseCountV2, pivots: ZigzagPivot[]): DiagonalRoleV2 {
  if (pivots.length < 4) return "unknown";
  const [p0, , , , , p5] = imp.pivots;
  const p0Rank = pivots.findIndex((p) => p.index === p0.index);
  const p5Rank = pivots.findIndex((p) => p.index === p5.index);
  if (p0Rank < 0 || p5Rank < 0) return "unknown";
  const n = pivots.length;
  const earlyStart = p0Rank <= 1;
  const lateEnd = p5Rank >= n - 2;
  if (earlyStart && !lateEnd) return "leading";
  if (lateEnd && !earlyStart) return "ending";
  return "unknown";
}
