/** `analysis_snapshots` / `signal_dashboard` JSON (Rust `SignalDashboardV1`). */
export type SignalDashboardPayload = {
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
