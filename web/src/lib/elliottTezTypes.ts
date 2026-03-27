/** Tez metni — satır/satır korunur; GUI ve statik txt ile paylaşılır. */
export type ElliottTezRuleLine = { id: string; text: string };

export type ElliottTezSection = {
  id: string;
  heading: string;
  paragraphs?: readonly string[];
  rulesHeading?: string;
  rules?: readonly ElliottTezRuleLine[];
  guidelinesHeading?: string;
  guidelines?: readonly ElliottTezRuleLine[];
  /** Madde işaretli kısa liste (ör. dört düzeltme tipi) */
  bullets?: readonly string[];
};
