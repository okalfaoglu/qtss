import type { ElliottEngineOutputV2 } from "./elliottEngineV2";

export type ElliottLegendRow = {
  id: string;
  swatch: "primary" | "sub" | "abc" | "projection" | "projection_target";
  title: string;
  detail: string;
};

/**
 * GUI lejantı — MTF V2 makro / ara / mikro dalgaları.
 * `showProjection`: İleri formasyon projeksiyonu açıksa ek satır.
 */
export function buildElliottLegendRows(
  out: ElliottEngineOutputV2 | null,
  showProjection = false,
  showProjectionTargets = false,
): ElliottLegendRow[] {
  const rows: ElliottLegendRow[] = [];
  const m = out?.hierarchy.macro ?? null;
  const i = out?.hierarchy.intermediate ?? null;
  const x = out?.hierarchy.micro ?? null;

  rows.push({
    id: "v2_macro",
    swatch: "primary",
    title: "4h Makro dalga",
    detail: !m
      ? "Veri yok veya hesaplanamadı."
      : m.impulse
        ? `Etiket ①..⑤ / Ⓐ..Ⓒ, karar: ${m.decision}. Makro (4h) — kalın çizgi; renk çekmede ayarlanır.`
        : `İtki bulunamadı, karar: ${m.decision}.`,
  });
  rows.push({
    id: "v2_intermediate",
    swatch: "sub",
    title: "1h Ara dalga",
    detail: !i
      ? "Veri yok veya hesaplanamadı."
      : i.impulse
        ? `Etiket (1)..(5) / (A)..(C), karar: ${i.decision}. Orta yeşil doğrulama katmanı.`
        : `İtki bulunamadı, karar: ${i.decision}.`,
  });
  rows.push({
    id: "v2_micro",
    swatch: "abc",
    title: "15m Mikro dalga",
    detail: !x
      ? "Veri yok veya hesaplanamadı."
      : x.impulse
        ? `Etiket i..v / a..c, karar: ${x.decision}. Mikro (15m) katmanı; renk çekmede ayarlanır.`
        : `İtki bulunamadı, karar: ${x.decision}.`,
  });

  if (showProjection) {
    rows.push({
      id: "projection",
      swatch: "projection",
      title: "İleri projeksiyon (formasyon)",
      detail: "ABC ve sonraki itki segmentleri şeması. Yatırım tavsiyesi değildir.",
    });
  }
  if (showProjectionTargets) {
    rows.push({
      id: "projection_targets",
      swatch: "projection_target",
      title: "Projeksiyon hedef seviyeleri",
      detail: "Formasyon projeksiyonunda (ABC) yatay hedef çizgileri ayrı stil ile gösterilir.",
    });
  }

  return rows;
}
