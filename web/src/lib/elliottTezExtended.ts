import { ELLIOTT_TEZ_2534_SECTIONS } from "./elliottTez2534";
import { ELLIOTT_TEZ_254_SECTIONS } from "./elliottTez254";
import { ELLIOTT_TEZ_255_SECTIONS } from "./elliottTez255";
import type { ElliottTezSection } from "./elliottTezTypes";

export type { ElliottTezRuleLine, ElliottTezSection } from "./elliottTezTypes";

/** Tez §2.5.3.4 – §2.5.5 — satır satır veri (panel + tek kaynak). */
export const ELLIOTT_TEZ_EXTENDED_SECTIONS: readonly ElliottTezSection[] = [
  ...ELLIOTT_TEZ_2534_SECTIONS,
  ...ELLIOTT_TEZ_254_SECTIONS,
  ...ELLIOTT_TEZ_255_SECTIONS,
];
