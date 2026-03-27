/**
 * Elliott dalga türleri — menü başlıkları + checkbox anahtarları.
 * Kurallar motor içinde: `impulse.ts`, `corrective.ts`, `tezWaveChecks.ts`.
 */

export type ElliottPatternMenuToggles = {
  motive_impulse: boolean;
  motive_diagonal: boolean;
  corrective_zigzag: boolean;
  corrective_flat: boolean;
  corrective_triangle: boolean;
  corrective_complex_wxy: boolean;
};

export type ElliottPatternMenuItem = {
  id: keyof ElliottPatternMenuToggles;
  titleTr: string;
  titleEn: string;
  structure?: string;
};

export const DEFAULT_ELLIOTT_PATTERN_MENU: ElliottPatternMenuToggles = {
  motive_impulse: true,
  // QTSS V2 standard: diagonal overlap exception is disabled by default.
  motive_diagonal: false,
  corrective_zigzag: true,
  corrective_flat: true,
  corrective_triangle: true,
  corrective_complex_wxy: true,
};

export type ElliottPatternMenuGroup = {
  id: string;
  titleTr: string;
  titleEn: string;
  items: readonly ElliottPatternMenuItem[];
};

export const ELLIOTT_PATTERN_MENU_GROUPS: readonly ElliottPatternMenuGroup[] = [
  {
    id: "motive",
    titleTr: "1. İTKİ (MOTIVE) DALGALARI",
    titleEn: "Motive waves",
    items: [
      {
        id: "motive_impulse",
        titleTr: "Impulse (İtme dalgası)",
        titleEn: "Impulse",
        structure: "5-3-5-3-5",
      },
      {
        id: "motive_diagonal",
        titleTr: "Diagonals (Diyagonallar)",
        titleEn: "Diagonals",
        structure: "Sonlanan 3-3-3-3-3 / ilerleyen 5-3-5-3-5",
      },
    ],
  },
  {
    id: "corrective",
    titleTr: "2. DÜZELTME (CORRECTIVE) DALGALARI",
    titleEn: "Corrective waves",
    items: [
      {
        id: "corrective_zigzag",
        titleTr: "Zigzag (Zikzak)",
        titleEn: "Zigzag",
        structure: "[5-3-5]",
      },
      {
        id: "corrective_flat",
        titleTr: "Flat (Yatay)",
        titleEn: "Flat",
        structure: "[3-3-5]",
      },
      {
        id: "corrective_triangle",
        titleTr: "Triangle (Üçgenler)",
        titleEn: "Triangle",
        structure: "[3-3-3-3-3]",
      },
      {
        id: "corrective_complex_wxy",
        titleTr: "Complex (Karmaşık — W-X-Y)",
        titleEn: "Complex correction",
        structure: "W-X-Y / W-X-Y-X-Z",
      },
    ],
  },
] as const;
