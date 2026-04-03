export type Timeframe = "15m" | "1h" | "4h" | "1d" | "1w";

import type { ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";

export type OhlcV2 = {
  t: number; // epoch seconds
  o: number;
  h: number;
  l: number;
  c: number;
};

export type ZigzagPivotKind = "high" | "low";

export type ZigzagPivot = {
  index: number;
  time: number; // epoch seconds
  price: number;
  kind: ZigzagPivotKind;
};

export type ZigzagParams = {
  depth: number;
  deviationPct: number;
  backstep: number;
};

export type ElliottDecisionClass = "invalid" | "candidate" | "confirmed";

export type ElliottRuleCheckV2 = {
  id: string;
  passed: boolean;
  detail?: string;
};

export type ImpulseDirectionV2 = "bull" | "bear";

/** Chart-window heuristic for diagonal subtype; not a higher-timeframe wave-degree label. */
export type DiagonalRoleV2 = "leading" | "ending" | "unknown";

/** How `diagonalRole` was chosen when `variant: diagonal`. */
export type DiagonalRoleSourceV2 = "chart_window" | "mtf_parent";

export type ImpulseCountV2 = {
  direction: ImpulseDirectionV2;
  pivots: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot];
  checks: ElliottRuleCheckV2[];
  score: number;
  /** Standart: w4–w1 bindirme yok. Diyagonal: bindirme serbest (§ sonlanan/ilerleyen diyagonal). */
  variant?: "standard" | "diagonal";
  /**
   * Diyagonal: uçların zigzag dizisindeki konumuna göre leading vs ending tahmini.
   * `unknown` → alt-dalga kanıtı ending (3-3-3-3-3) varsayımı ile yapılır.
   */
  diagonalRole?: DiagonalRoleV2;
  /** Set when `variant: diagonal` after role resolution (chart heuristic and/or MTF parent overlap). */
  diagonalRoleSource?: DiagonalRoleSourceV2;
};

export type CorrectivePatternV2 = "zigzag" | "flat" | "triangle" | "combination" | "abc";

export type CorrectiveCountV2 = {
  pivots: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot];
  /** For non-ABC shapes (triangle/combination), optional explicit path used for drawing. */
  path?: ZigzagPivot[];
  /** Optional label sequence aligned with `path` or `pivots`. */
  labels?: string[];
  pattern: CorrectivePatternV2;
  checks: ElliottRuleCheckV2[];
  score: number;
};

export type TimeframeStateV2 = {
  timeframe: Timeframe;
  pivots: ZigzagPivot[];
  impulse: ImpulseCountV2 | null;
  /**
   * Dalga 1 bacağı (ana itkı p0→p1) içinde, aynı zigzag pivotları üzerinde bulunan alt derece 5’li itkı.
   * Pivot sayısı yetersizse veya uçlar hizalanmazsa `null`.
   */
  wave1NestedImpulse: ImpulseCountV2 | null;
  /** Dalga 2 (p1–p2): mikro zigzag üzerinde düzeltme (zigzag/flat/…) — ana wave2’den bağımsız ince katman. */
  wave2NestedCorrective: CorrectiveCountV2 | null;
  /** Dalga 3 (p2–p3): alt derece 5’li itkı. */
  wave3NestedImpulse: ImpulseCountV2 | null;
  /** Dalga 4 (p3–p4): mikro zigzag üzerinde düzeltme. */
  wave4NestedCorrective: CorrectiveCountV2 | null;
  /** Dalga 5 (p4–p5): alt derece 5’li itkı. */
  wave5NestedImpulse: ImpulseCountV2 | null;
  /** Geçmişte bulunan ek itki adayları (çakışmasız, yeniye yakın öncelik). */
  historicalImpulses?: ImpulseCountV2[];
  wave2: CorrectiveCountV2 | null;
  wave4: CorrectiveCountV2 | null;
  postImpulseAbc: CorrectiveCountV2 | null;
  decision: ElliottDecisionClass;
};

export type ElliottEngineInputV2 = {
  byTimeframe: Partial<Record<Timeframe, OhlcV2[]>>;
  zigzag: ZigzagParams;
  /** Varsa her TF için `zigzag.depth` yerine bu değer kullanılır. */
  zigzagDepthByTimeframe?: Partial<Record<Timeframe, number>>;
  maxWindows?: number;
  /** Düzeltme dalgası kalıplarını filtreler; yoksa hepsi açık kabul edilir. */
  patternToggles?: ElliottPatternMenuToggles;
  /** TF başına dalga türleri; varsa `patternToggles` yerine kullanılır. */
  patternTogglesByTf?: Partial<Record<Timeframe, ElliottPatternMenuToggles>>;
};

export type ElliottEngineOutputV2 = {
  states: Partial<Record<Timeframe, TimeframeStateV2>>;
  hierarchy: {
    macro: TimeframeStateV2 | null;
    intermediate: TimeframeStateV2 | null;
    micro: TimeframeStateV2 | null;
  };
  /** Motor girdisi — ham zigzag çizgisini son uca uzatmak için (pivot dizisi değişmez). */
  ohlcByTf?: Partial<Record<Timeframe, OhlcV2[]>>;
  /** Grafik uzatması için zigzag parametreleri (motor ile aynı). */
  zigzagParams: ZigzagParams;
  /** TF başına efektif zigzag (depth farklıysa). */
  zigzagParamsByTf?: Partial<Record<Timeframe, ZigzagParams>>;
};

