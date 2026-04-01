import { useTranslation } from "react-i18next";
import type { EngineSnapshotJoinedApiRow } from "../api/client";
import {
  formatDashboardNumber,
  parseSignalDashboardV2,
  pickDashboardBool,
  pickDashboardNum,
  pickDashboardStr,
  type SignalDashboardPayload,
} from "../lib/signalDashboardPayload";

type Props = {
  snapshot: EngineSnapshotJoinedApiRow | null;
};

/**
 * `analysis_snapshots` row `engine_kind === "signal_dashboard"` for the chart-matched engine target.
 */
export function SignalDashboardDrawerPanel({ snapshot }: Props) {
  const { t } = useTranslation();

  if (!snapshot) {
    return (
      <p className="muted" style={{ fontSize: "0.75rem" }}>
        {t("app.signalDashboardDrawer.noSnapshotForChart")}
      </p>
    );
  }

  const symbolUpper = snapshot.symbol?.trim() ? snapshot.symbol.trim().toUpperCase() : "—";
  const venueLine = `${snapshot.exchange?.trim() || "—"}/${snapshot.segment?.trim() || "—"} · ${snapshot.interval?.trim() || "—"}`;

  const instrumentBanner = (
    <div className="mono muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
      <div>
        <span className="muted">{t("app.signalDashboard.row.symbol")}: </span>
        <strong style={{ color: "var(--fg, inherit)" }}>{symbolUpper}</strong>
      </div>
      <div style={{ marginTop: "0.08rem" }}>{venueLine}</div>
    </div>
  );

  if (snapshot.error) {
    return (
      <>
        {instrumentBanner}
        <p className="err" style={{ fontSize: "0.75rem" }}>
          {snapshot.error}
        </p>
      </>
    );
  }

  const raw = snapshot.payload;
  if (!raw || typeof raw !== "object") {
    return (
      <>
        {instrumentBanner}
        <p className="muted" style={{ fontSize: "0.75rem" }}>
          {t("app.signalDashboard.payloadEmpty")}
        </p>
      </>
    );
  }

  const ins = raw as Record<string, unknown>;
  if (ins.reason === "insufficient_bars") {
    return (
      <>
        {instrumentBanner}
        <p className="muted" style={{ fontSize: "0.75rem" }}>
          {t("app.signalDashboard.insufficientBars")}
        </p>
      </>
    );
  }

  const p = raw as SignalDashboardPayload;
  const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
  const rk = (key: string, v: string) => (
    <tr key={key}>
      <td className="muted" style={{ padding: "0.12rem 0.35rem 0.12rem 0", verticalAlign: "top" }}>
        {t(`app.signalDashboard.row.${key}`)}
      </td>
      <td className="mono" style={{ padding: "0.12rem 0", wordBreak: "break-all" }}>
        {v}
      </td>
    </tr>
  );
  const yn = (b: boolean | undefined) => (b ? t("app.signalDashboard.ynYes") : t("app.signalDashboard.ynNo"));
  const posStr =
    v2?.position_strength_10 != null
      ? `${v2.position_strength_10} / 10`
      : p.pozisyon_gucu_10 != null
        ? `${p.pozisyon_gucu_10} / 10`
        : "—";
  const sysStr =
    pickDashboardBool(v2?.system_active, p.sistem_aktif) === true
      ? t("app.signalDashboard.systemActive")
      : "—";
  const wireRow = (key: string, val: unknown) => {
    if (val === undefined || val === null) return null;
    const s = typeof val === "boolean" ? (val ? "true" : "false") : String(val);
    return (
      <tr key={key}>
        <td
          className="muted mono"
          style={{ padding: "0.08rem 0.35rem 0.08rem 0", verticalAlign: "top", width: "42%" }}
        >
          {key}
        </td>
        <td className="mono" style={{ padding: "0.08rem 0", wordBreak: "break-all" }}>
          {s}
        </td>
      </tr>
    );
  };

  const payloadSymbol =
    typeof ins.symbol === "string" && ins.symbol.trim() ? ins.symbol.trim().toUpperCase() : null;
  const displaySymbol = payloadSymbol ?? symbolUpper;

  return (
    <>
      <p className="muted" style={{ fontSize: "0.66rem", marginBottom: "0.35rem" }}>
        {t("app.signalDashboard.priorityLine")}
      </p>
      <table style={{ width: "100%", fontSize: "0.74rem", borderCollapse: "collapse" }}>
        <tbody>
          {rk("symbol", displaySymbol)}
          {rk("venueInterval", venueLine)}
          {rk("status", pickDashboardStr(v2?.status, p.durum))}
          {rk("statusModelRaw", pickDashboardStr(v2?.status_model_raw, p.durum_model_raw))}
          {rk("directionPolicyDb", p.signal_direction_mode ?? "—")}
          {rk("directionEffective", p.signal_direction_effective ?? "—")}
          {rk("localTrend", pickDashboardStr(v2?.local_trend, p.yerel_trend))}
          {rk("globalTrend", pickDashboardStr(v2?.global_trend, p.global_trend))}
          {rk("marketMode", pickDashboardStr(v2?.market_mode, p.piyasa_modu))}
          {rk("entryMode", pickDashboardStr(v2?.entry_mode, p.giris_modu))}
          {rk(
            "volatilityPct",
            v2?.volatility_pct != null && Number.isFinite(v2.volatility_pct)
              ? v2.volatility_pct.toFixed(2)
              : p.oynaklik_pct != null
                ? p.oynaklik_pct.toFixed(2)
                : "—",
          )}
          {rk("momentum1", pickDashboardStr(v2?.momentum_rsi, p.momentum_1))}
          {rk("momentum2", pickDashboardStr(v2?.momentum_roc, p.momentum_2))}
          {rk("entryActual", pickDashboardNum(v2?.entry_price ?? undefined, p.giris_gercek ?? undefined))}
          {rk("stopInitial", pickDashboardNum(v2?.stop_initial ?? undefined, p.stop_ilk ?? undefined))}
          {rk(
            "takeProfitInitial",
            pickDashboardNum(v2?.take_profit_initial ?? undefined, p.kar_al_ilk ?? undefined),
          )}
          {rk(
            "stopTrailActive",
            pickDashboardNum(v2?.stop_trail ?? undefined, p.stop_trail_aktif ?? undefined),
          )}
          {rk(
            "takeProfitDynamic",
            pickDashboardNum(v2?.take_profit_dynamic ?? undefined, p.kar_al_dinamik ?? undefined),
          )}
          {rk("signalSource", pickDashboardStr(v2?.signal_source, p.sinyal_kaynagi))}
          {rk("trendExhaustion", yn(pickDashboardBool(v2?.trend_exhaustion, p.trend_tukenmesi)))}
          {rk("structureShift", yn(pickDashboardBool(v2?.structure_shift, p.yapi_kaymasi)))}
          {rk("positionStrength", posStr)}
          {rk("system", sysStr)}
          {rk("rangeHigh", formatDashboardNumber(p.range_high ?? undefined))}
          {rk("rangeLow", formatDashboardNumber(p.range_low ?? undefined))}
          {rk("rangeMid", formatDashboardNumber(p.range_mid ?? undefined))}
          {rk("atr", formatDashboardNumber(p.atr ?? undefined))}
          {rk("lastBar", p.last_bar_open_time ?? "—")}
        </tbody>
      </table>
      {v2 ? (
        <details style={{ marginTop: "0.45rem" }}>
          <summary className="muted" style={{ fontSize: "0.7rem", cursor: "pointer" }}>
            {t("app.signalDashboard.wireSummary")} <code>signal_dashboard_v2</code>
          </summary>
          <table
            style={{ width: "100%", fontSize: "0.68rem", borderCollapse: "collapse", marginTop: "0.28rem" }}
            className="mono muted"
          >
            <tbody>
              {wireRow("schema_version", v2.schema_version)}
              {wireRow("status", v2.status)}
              {wireRow("status_model_raw", v2.status_model_raw)}
              {wireRow("local_trend", v2.local_trend)}
              {wireRow("global_trend", v2.global_trend)}
              {wireRow("market_mode", v2.market_mode)}
              {wireRow("entry_mode", v2.entry_mode)}
              {wireRow("volatility_pct", v2.volatility_pct)}
              {wireRow("momentum_rsi", v2.momentum_rsi)}
              {wireRow("momentum_roc", v2.momentum_roc)}
              {wireRow("entry_price", v2.entry_price)}
              {wireRow("stop_initial", v2.stop_initial)}
              {wireRow("take_profit_initial", v2.take_profit_initial)}
              {wireRow("stop_trail", v2.stop_trail)}
              {wireRow("take_profit_dynamic", v2.take_profit_dynamic)}
              {wireRow("signal_source", v2.signal_source)}
              {wireRow("trend_exhaustion", v2.trend_exhaustion)}
              {wireRow("structure_shift", v2.structure_shift)}
              {wireRow("position_strength_10", v2.position_strength_10)}
              {wireRow("system_active", v2.system_active)}
            </tbody>
          </table>
        </details>
      ) : null}
    </>
  );
}
