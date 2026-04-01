/**
 * Elliott wave pattern menu — hierarchical labels + per-leaf toggles.
 * Rules engine: `impulse.ts`, `corrective.ts`, `tezWaveChecks.ts`.
 */

export type ElliottPatternMenuToggles = {
  motive_impulse: boolean;
  motive_diagonal_leading: boolean;
  motive_diagonal_ending: boolean;
  corrective_zigzag: boolean;
  corrective_flat: boolean;
  corrective_complex_double: boolean;
  corrective_complex_triple: boolean;
  corrective_triangle: boolean;
};

export type ElliottPatternMenuRow =
  | { type: "label"; id: string; titleTr: string; titleEn: string; depth: number; section: "motive" | "corrective" }
  | {
      type: "toggle";
      id: keyof ElliottPatternMenuToggles;
      titleTr: string;
      titleEn: string;
      structure?: string;
      depth: number;
      section: "motive" | "corrective";
    };

export const DEFAULT_ELLIOTT_PATTERN_MENU: ElliottPatternMenuToggles = {
  motive_impulse: true,
  motive_diagonal_leading: true,
  motive_diagonal_ending: true,
  corrective_zigzag: true,
  corrective_flat: true,
  corrective_complex_double: true,
  corrective_complex_triple: true,
  corrective_triangle: true,
};

/** Table / panel: section headers (label) and leaf toggles in tree order. */
export const ELLIOTT_PATTERN_MENU_ROWS: readonly ElliottPatternMenuRow[] = [
  { type: "label", id: "lbl_impulse", titleTr: "Impulse", titleEn: "Impulse", depth: 0, section: "motive" },
  {
    type: "toggle",
    id: "motive_impulse",
    titleTr: "Normal Impulse (1-2-3-4-5)",
    titleEn: "Normal impulse (1-2-3-4-5)",
    structure: "5-3-5-3-5",
    depth: 1,
    section: "motive",
  },
  { type: "label", id: "lbl_diagonal", titleTr: "Diagonal", titleEn: "Diagonal", depth: 1, section: "motive" },
  {
    type: "toggle",
    id: "motive_diagonal_leading",
    titleTr: "Leading Diagonal",
    titleEn: "Leading diagonal",
    structure: "5-3-5-3-5",
    depth: 2,
    section: "motive",
  },
  {
    type: "toggle",
    id: "motive_diagonal_ending",
    titleTr: "Ending Diagonal",
    titleEn: "Ending diagonal",
    structure: "3-3-3-3-3",
    depth: 2,
    section: "motive",
  },
  {
    type: "label",
    id: "lbl_corrective",
    titleTr: "Corrective (ABC)",
    titleEn: "Corrective (ABC)",
    depth: 0,
    section: "corrective",
  },
  { type: "label", id: "lbl_simple", titleTr: "Simple", titleEn: "Simple", depth: 1, section: "corrective" },
  {
    type: "toggle",
    id: "corrective_zigzag",
    titleTr: "Zigzag",
    titleEn: "Zigzag",
    structure: "[5-3-5]",
    depth: 2,
    section: "corrective",
  },
  {
    type: "toggle",
    id: "corrective_flat",
    titleTr: "Flat",
    titleEn: "Flat",
    structure: "[3-3-5]",
    depth: 2,
    section: "corrective",
  },
  { type: "label", id: "lbl_complex", titleTr: "Complex", titleEn: "Complex", depth: 1, section: "corrective" },
  {
    type: "toggle",
    id: "corrective_complex_double",
    titleTr: "Double Zigzag (W-X-Y)",
    titleEn: "Double zigzag (W-X-Y)",
    depth: 2,
    section: "corrective",
  },
  {
    type: "toggle",
    id: "corrective_complex_triple",
    titleTr: "Triple Zigzag (W-X-Y-X-Z)",
    titleEn: "Triple zigzag (W-X-Y-X-Z)",
    depth: 2,
    section: "corrective",
  },
  {
    type: "toggle",
    id: "corrective_triangle",
    titleTr: "Triangle (A-B-C-D-E)",
    titleEn: "Triangle (A-B-C-D-E)",
    structure: "[3-3-3-3-3]",
    depth: 1,
    section: "corrective",
  },
] as const;

/** Collapsible panel: one “select all” per top-level branch. */
export const ELLIOTT_PATTERN_MENU_PANEL_SECTIONS: readonly {
  id: "motive" | "corrective";
  titleTr: string;
  titleEn: string;
  toggleIds: readonly (keyof ElliottPatternMenuToggles)[];
}[] = [
  {
    id: "motive",
    titleTr: "Impulse",
    titleEn: "Impulse",
    toggleIds: ["motive_impulse", "motive_diagonal_leading", "motive_diagonal_ending"],
  },
  {
    id: "corrective",
    titleTr: "Corrective (ABC)",
    titleEn: "Corrective (ABC)",
    toggleIds: [
      "corrective_zigzag",
      "corrective_flat",
      "corrective_complex_double",
      "corrective_complex_triple",
      "corrective_triangle",
    ],
  },
] as const;

export function patternMenuAllowDiagonal(menu: ElliottPatternMenuToggles): boolean {
  return menu.motive_diagonal_leading || menu.motive_diagonal_ending;
}

/** W–X–Y–X–Z path uses five letter labels; W–X–Y uses three. */
export function correctiveCombinationIsTriple(c: { labels?: string[] }): boolean {
  return (c.labels?.length ?? 0) > 3;
}
