import type { EngineSnapshotJoinedApiRow } from "../api/client";

/** `analysis_snapshots` / `signal_dashboard` JSON (Rust `SignalDashboardV1`). */
export type SignalDashboardPayload = {
  /** Worker `attach_engine_context` — JOIN satırıyla aynı olmalı. */
  symbol?: string;
  exchange?: string;
  segment?: string;
  interval?: string;
  schema_version?: number;
  durum?: string;
  /** Politika öncesi ham model (LONG/SHORT/NOTR). */
  durum_model_raw?: string;
  /** DB `engine_symbols.signal_direction_mode`. */
  signal_direction_mode?: string;
  /** Worker’ın uyguladığı etkin politika: both | long_only | short_only. */
  signal_direction_effective?: string;
  yerel_trend?: string;
  global_trend?: string;
  piyasa_modu?: string;
  giris_modu?: string;
  oynaklik_pct?: number;
  momentum_1?: string;
  momentum_2?: string;
  giris_gercek?: number | null;
  stop_ilk?: number | null;
  kar_al_ilk?: number | null;
  stop_trail_aktif?: number | null;
  kar_al_dinamik?: number | null;
  sinyal_kaynagi?: string;
  trend_tukenmesi?: boolean;
  yapi_kaymasi?: boolean;
  pozisyon_gucu_10?: number;
  sistem_aktif?: boolean;
  last_bar_open_time?: string;
  /** Worker `enrich_dashboard_payload` — TR ile aynı pencere. */
  range_high?: number;
  range_low?: number;
  range_mid?: number;
  atr?: number;
};

export function formatDashboardNumber(n: number | null | undefined): string {
  if (n == null || typeof n !== "number" || !Number.isFinite(n)) return "—";
  return n.toFixed(4);
}

/** Worker `enrich_dashboard_payload` — PLAN Phase F, `schema_version` 3. */
export type SignalDashboardV2Payload = {
  schema_version?: number;
  status?: string;
  status_model_raw?: string;
  local_trend?: string;
  global_trend?: string;
  market_mode?: string;
  entry_mode?: string;
  volatility_pct?: number;
  momentum_rsi?: string;
  momentum_roc?: string;
  entry_price?: number | null;
  stop_initial?: number | null;
  take_profit_initial?: number | null;
  stop_trail?: number | null;
  take_profit_dynamic?: number | null;
  signal_source?: string;
  trend_exhaustion?: boolean;
  structure_shift?: boolean;
  position_strength_10?: number;
  system_active?: boolean;
};

export function parseSignalDashboardV2(raw: unknown): SignalDashboardV2Payload | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const ver = o.schema_version;
  if (typeof ver !== "number" || ver !== 3) return null;
  return raw as SignalDashboardV2Payload;
}

export function pickDashboardStr(v2: string | undefined, v1: string | undefined): string {
  const s = v2?.trim();
  if (s) return s;
  return v1 ?? "—";
}

export function pickDashboardNum(
  v2: number | null | undefined,
  v1: number | null | undefined,
): string {
  const n =
    v2 != null && typeof v2 === "number" && Number.isFinite(v2)
      ? v2
      : v1 != null && typeof v1 === "number" && Number.isFinite(v1)
        ? v1
        : undefined;
  return formatDashboardNumber(n);
}

export function pickDashboardBool(v2: boolean | undefined, v1: boolean | undefined): boolean | undefined {
  if (typeof v2 === "boolean") return v2;
  return v1;
}

/** List row border / accent from effective status (and payload health). */
export type SignalDashboardRowAccent = "long" | "short" | "neutral" | "error" | "insufficient";

export function signalDashboardRowAccent(snapshot: EngineSnapshotJoinedApiRow): SignalDashboardRowAccent {
  if (snapshot.error?.trim()) return "error";
  const raw = snapshot.payload;
  if (!raw || typeof raw !== "object") return "neutral";
  const ins = raw as Record<string, unknown>;
  if (ins.reason === "insufficient_bars") return "insufficient";
  const p = raw as SignalDashboardPayload;
  const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
  const status = pickDashboardStr(v2?.status, p.durum).trim().toUpperCase();
  if (status === "LONG") return "long";
  if (status === "SHORT") return "short";
  return "neutral";
}

/** Detail table cell coloring (semantic, works with EN/TR display strings). */
export type DashboardValueTone = "default" | "bull" | "bear" | "muted" | "warn" | "accent" | "stop";

function classifyStatusDisplay(valueStr: string): DashboardValueTone {
  const u = valueStr.trim().toUpperCase();
  if (u === "LONG") return "bull";
  if (u === "SHORT") return "bear";
  if (u === "NOTR" || u === "NEUTRAL" || u === "NÖTR") return "muted";
  return "default";
}

function classifyTrendDisplay(valueStr: string): DashboardValueTone {
  const v = valueStr.trim().toLowerCase();
  if (v === "up" || v === "yukarı" || v === "yukari") return "bull";
  if (v === "down" || v === "aşağı" || v === "asagi") return "bear";
  if (v === "off" || v === "closed" || v === "kapalı" || v === "kapali" || v === "none") return "muted";
  return "default";
}

function classifyMomentumDisplay(valueStr: string): DashboardValueTone {
  const v = valueStr.trim().toLowerCase();
  if (v.includes("positive") || v.includes("pozitif")) return "bull";
  if (v.includes("negative") || v.includes("negatif")) return "bear";
  if (v.includes("neutral") || v.includes("nötr") || v.includes("notr")) return "muted";
  return "default";
}

/**
 * Maps a translated row label key (`app.signalDashboard.row.*`) plus rendered value to a tone.
 * Optional `toneOverride` skips inference (e.g. booleans).
 */
export function dashboardValueTone(rowKey: string, valueStr: string): DashboardValueTone {
  if (valueStr === "—") return "default";
  const v = valueStr.trim().toLowerCase();
  switch (rowKey) {
    case "symbol":
    case "venueInterval":
    case "directionPolicyDb":
    case "directionEffective":
    case "volatilityPct":
    case "rangeHigh":
    case "rangeLow":
    case "rangeMid":
    case "atr":
    case "lastBar":
      return "default";
    case "entryActual":
      return "warn";
    case "stopInitial":
    case "stopTrailActive":
      return "stop";
    case "takeProfitInitial":
    case "takeProfitDynamic":
      return "bull";
    case "status":
    case "statusModelRaw":
      return classifyStatusDisplay(valueStr);
    case "localTrend":
    case "globalTrend":
      return classifyTrendDisplay(valueStr);
    case "marketMode": {
      if (v.includes("break") || v.includes("kopuş") || v.includes("kopus")) return "warn";
      return "default";
    }
    case "entryMode": {
      if (v.includes("reversal") || v.includes("dönüş") || v.includes("donus")) return "accent";
      return "default";
    }
    case "momentum1":
    case "momentum2":
      return classifyMomentumDisplay(valueStr);
    case "signalSource":
      return "accent";
    case "trendExhaustion":
    case "structureShift":
      return "default";
    case "positionStrength":
      return "default";
    case "system":
      return "default";
    default:
      return "default";
  }
}
