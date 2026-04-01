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
