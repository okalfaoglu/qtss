/**
 * §2.5.4 (`elliottRulesCatalog.ts` — flat/zigzag kuralları) ile hizalı — ABC düzeltme için ölçülebilir kural kontrolleri.
 * Paragraf metinleri GUI’de; burada yalnızca sayısal eşikler uygulanır.
 */

import type { ElliottRuleCheckV2, ZigzagPivot } from "./types";

/**
 * Tez §2.5.4.1 `flat_r4` — genişletilmiş yassı dahil B/A alt sınırı (0.382).
 * Klasik “düzenli yassı”da B genelde A’nın ~%61.8’inden fazlasını geri alır; bu motor
 * düşük oranları **expanded flat** ile uyum için kabul eder. `classifyFlatVsZigzag` (0.9)
 * ile “flat” etiketi pratikte yüksek B/A’ya kayar.
 */
export const TEZ_FLAT_B_VS_A_MIN = 0.382;
export const TEZ_FLAT_B_VS_A_MAX = 2.618;

/** Klasik düzenli yassı için tipik B/A tabanı (referans; `flat_r4` ile karıştırma). */
export const TEZ_CLASSIC_REGULAR_FLAT_B_MIN = 0.618;

/** Tez §2.5.4.1 flat_g7 — C en az A dalgasının 0.382 katı kadar olmalıdır. */
export const TEZ_FLAT_C_VS_A_MIN = 0.382;

/** Tez §2.5.4.2 zz_r5 — B/A alt sınırı (zigzag). */
export const TEZ_ZIGZAG_B_VS_A_MIN = 0.382;

/** Tez §2.5.4.2 zz_r5 — B, A dalgasının %61.8’inden fazlasını asla geri alamaz. */
export const TEZ_ZIGZAG_B_VS_A_MAX = 0.618;

/**
 * Yassı / zigzag ABC için tez §2.5.4.1–2 kurallarından motor kontrolleri.
 * `impulseBull`: ana itki yönü (dalga 2/4 düzeltmesi ters yöndedir).
 */
export function buildTez254AbcChecks(
  pattern: "zigzag" | "flat",
  retrB: number,
  cVsA: number,
  impulseBull: boolean,
  start: ZigzagPivot,
  a: ZigzagPivot,
  b: ZigzagPivot,
  end: ZigzagPivot,
): ElliottRuleCheckV2[] {
  const checks: ElliottRuleCheckV2[] = [];

  if (pattern === "flat") {
    const flatB = retrB >= TEZ_FLAT_B_VS_A_MIN && retrB <= TEZ_FLAT_B_VS_A_MAX;
    checks.push({
      id: "flat_r4",
      passed: flatB,
      detail: `B/A=${retrB.toFixed(3)} ∈ [${TEZ_FLAT_B_VS_A_MIN}, ${TEZ_FLAT_B_VS_A_MAX}] (düzenli yassı tipik ≥${TEZ_CLASSIC_REGULAR_FLAT_B_MIN})`,
    });
    checks.push({
      id: "flat_g7",
      passed: cVsA >= TEZ_FLAT_C_VS_A_MIN - 1e-12,
      detail: `C/A=${cVsA.toFixed(3)} ≥ ${TEZ_FLAT_C_VS_A_MIN}`,
    });
  } else {
    checks.push({
      id: "zz_r5",
      passed: retrB >= TEZ_ZIGZAG_B_VS_A_MIN - 1e-12 && retrB <= TEZ_ZIGZAG_B_VS_A_MAX + 1e-12,
      detail: `B/A=${retrB.toFixed(3)} ∈ [${TEZ_ZIGZAG_B_VS_A_MIN}, ${TEZ_ZIGZAG_B_VS_A_MAX}]`,
    });

    /** zz_r1 — C ve B bacak uzunlukları: C = B→C, B = A→B (`|a−end|` eski hata: C bacağını kaçırıyordu). */
    const lenB = Math.abs(b.price - a.price);
    const lenC = Math.abs(b.price - end.price);
    checks.push({
      id: "zz_r1",
      passed: lenC + 1e-10 >= lenB,
      detail: `|C|=${lenC.toFixed(4)} ≥ |B|=${lenB.toFixed(4)}`,
    });

    const cBeyondA =
      impulseBull ? end.price < a.price - 1e-10 : end.price > a.price + 1e-10;
    checks.push({
      id: "zz_r6",
      passed: cBeyondA,
      detail: impulseBull ? `C_end< A_ucu (${end.price.toFixed(4)} < ${a.price.toFixed(4)})` : `C_end> A_ucu`,
    });

    const bNotBeyondAOrigin =
      impulseBull ? b.price <= start.price + 1e-10 : b.price >= start.price - 1e-10;
    checks.push({
      id: "zz_b_not_beyond_a_start",
      passed: bNotBeyondAOrigin,
      detail: impulseBull ? `B≤ A başlangıcı` : `B≥ A başlangıcı`,
    });
  }

  return checks;
}
