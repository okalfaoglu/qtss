import type { ChannelSixOutcomeJson } from "../api/client";

/** Pivotların mum indeksi aralığı (API `bar_index` ile uyumlu). */
export function outcomePivotBarRange(outcome: ChannelSixOutcomeJson): { min: number; max: number } | null {
  const pivots = outcome.pivots;
  if (!pivots?.length) return null;
  let min = pivots[0][0];
  let max = pivots[0][0];
  for (const p of pivots) {
    const b = p[0];
    min = Math.min(min, b);
    max = Math.max(max, b);
  }
  return { min, max };
}

/**
 * Alım-satım robotu için “şu anki seriye göre en güncel formasyon penceresi”.
 * - `pivot_tail_skip === 0`: Zigzag’da en yeni ardışık 6 (veya 5) pivot kullanıldı.
 * - `zigzag_level === 0` (veya yok): özyinelemeli üst seviye değil, temel zigzag.
 */
export function isLiveRobotSignal(outcome: ChannelSixOutcomeJson): boolean {
  const skip = outcome.pivot_tail_skip ?? 0;
  const lvl = outcome.zigzag_level ?? 0;
  return skip === 0 && lvl === 0;
}
