import { useEffect, useState } from "react";
import { useTranslation, type TFunction } from "react-i18next";
import type { EngineSnapshotJoinedApiRow } from "../api/client";
import {
  dashboardValueTone,
  formatDashboardNumber,
  hasExecutableSignalSetupLevels,
  parseSignalDashboardV2,
  pickDashboardBool,
  pickDashboardNum,
  pickDashboardStr,
  positionScenarioToneFromKind,
  scoreTrendToneFromKind,
  signalDashboardRowAccent,
  type DashboardValueTone,
  type SignalDashboardPayload,
} from "../lib/signalDashboardPayload";
import {
  formatDetectionPanel,
  formatEntryModePanel,
  formatMarketModePanel,
  formatMomentumPanel,
  formatTrendAxisPanel,
  formatTrendSignalSourcePanel,
  formatVolatilityPercentPanel,
  pickRsi14Last,
} from "../lib/signalDashboardPanelDisplay";

type Props = {
  snapshots: EngineSnapshotJoinedApiRow[];
  chartMatchedEngineSymbolId: string | null;
};

function formatScoreTrendDisplay(
  t: TFunction,
  kind: string | undefined,
  action: string | undefined,
): string {
  if (!kind?.trim()) return "—";
  const k = t(`app.signalDashboard.scoreTrend.kind.${kind}`, { defaultValue: kind });
  if (!action?.trim()) return k;
  const a = t(`app.signalDashboard.scoreTrend.action.${action}`, { defaultValue: action });
  return `${k} — ${a}`;
}

function formatScenarioDisplay(t: TFunction, kind: string | undefined): string {
  if (!kind?.trim() || kind === "none") return "—";
  return t(`app.signalDashboard.scoreTrend.scenario.${kind}`, { defaultValue: kind });
}

function accentClass(accent: ReturnType<typeof signalDashboardRowAccent>): string {
  switch (accent) {
    case "long":
      return "signal-dash-list__row--long";
    case "short":
      return "signal-dash-list__row--short";
    case "error":
      return "signal-dash-list__row--error";
    case "insufficient":
      return "signal-dash-list__row--insufficient";
    default:
      return "signal-dash-list__row--neutral";
  }
}

function valueClassName(tone: DashboardValueTone): string {
  if (tone === "default") return "mono signal-dash-val";
  return `mono signal-dash-val signal-dash-val--${tone}`;
}

function listRowSecondary(
  snapshot: EngineSnapshotJoinedApiRow,
  t: (key: string) => string,
): {
  status: string;
  localTrend: string;
  positionStrength: string;
} {
  if (snapshot.error?.trim()) {
    return { status: "—", localTrend: "—", positionStrength: "—" };
  }
  const raw = snapshot.payload;
  if (!raw || typeof raw !== "object") {
    return { status: "—", localTrend: "—", positionStrength: "—" };
  }
  const ins = raw as Record<string, unknown>;
  if (ins.reason === "insufficient_bars") {
    return { status: "—", localTrend: "—", positionStrength: "—" };
  }
  const p = raw as SignalDashboardPayload;
  const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
  const posStr =
    v2?.position_strength_10 != null
      ? `${v2.position_strength_10} / 10`
      : p.pozisyon_gucu_10 != null
        ? `${p.pozisyon_gucu_10} / 10`
        : "—";
  return {
    status: pickDashboardStr(v2?.status, p.durum),
    localTrend: formatTrendAxisPanel(t, v2?.local_trend, p.yerel_trend),
    positionStrength: posStr,
  };
}

function SignalDashboardDetailBody({ snapshot }: { snapshot: EngineSnapshotJoinedApiRow }) {
  const { t } = useTranslation();

  const symbolUpper = snapshot.symbol?.trim() ? snapshot.symbol.trim().toUpperCase() : "—";
  const venueLine = `${snapshot.exchange?.trim() || "—"}/${snapshot.segment?.trim() || "—"} · ${snapshot.interval?.trim() || "—"}`;

  const posStrForBanner = (): string => {
    if (snapshot.error?.trim() || !snapshot.payload || typeof snapshot.payload !== "object") return "—";
    const ins = snapshot.payload as Record<string, unknown>;
    if (ins.reason === "insufficient_bars") return "—";
    const p = snapshot.payload as SignalDashboardPayload;
    const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
    if (v2?.position_strength_10 != null) return `${v2.position_strength_10} / 10`;
    if (p.pozisyon_gucu_10 != null) return `${p.pozisyon_gucu_10} / 10`;
    return "—";
  };

  const rk = (key: string, v: string, toneOverride?: DashboardValueTone) => {
    const tone = toneOverride ?? dashboardValueTone(key, v);
    return (
      <tr key={key}>
        <td className="muted" style={{ padding: "0.12rem 0.35rem 0.12rem 0", verticalAlign: "top" }}>
          {t(`app.signalDashboard.row.${key}`)}
        </td>
        <td className={valueClassName(tone)} style={{ padding: "0.12rem 0", wordBreak: "break-all" }}>
          {v}
        </td>
      </tr>
    );
  };

  if (snapshot.error) {
    return (
      <>
        <div className="signal-dash-detail__banner">
          <div className="signal-dash-detail__banner-left mono">{symbolUpper}</div>
          <div className="signal-dash-detail__banner-right mono">{t("app.signalDashboardDrawer.strengthShort", { value: posStrForBanner() })}</div>
        </div>
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
        <div className="signal-dash-detail__banner">
          <div className="signal-dash-detail__banner-left mono">{symbolUpper}</div>
          <div className="signal-dash-detail__banner-right mono">{t("app.signalDashboardDrawer.strengthShort", { value: posStrForBanner() })}</div>
        </div>
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
        <div className="signal-dash-detail__banner">
          <div className="signal-dash-detail__banner-left mono">{symbolUpper}</div>
          <div className="signal-dash-detail__banner-right mono">{t("app.signalDashboardDrawer.strengthShort", { value: posStrForBanner() })}</div>
        </div>
        <p className="muted" style={{ fontSize: "0.75rem" }}>
          {t("app.signalDashboard.insufficientBars")}
        </p>
      </>
    );
  }

  const p = raw as SignalDashboardPayload;
  const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
  const rsi14 = pickRsi14Last(ins, v2, p);
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
  const sysTone: DashboardValueTone =
    pickDashboardBool(v2?.system_active, p.sistem_aktif) === true ? "bull" : "muted";
  const te = pickDashboardBool(v2?.trend_exhaustion, p.trend_tukenmesi);
  const ss = pickDashboardBool(v2?.structure_shift, p.yapi_kaymasi);
  const psNum = v2?.position_strength_10 ?? p.pozisyon_gucu_10;
  const psTone: DashboardValueTone =
    psNum != null && typeof psNum === "number" && Number.isFinite(psNum)
      ? psNum >= 7
        ? "bull"
        : psNum <= 3
          ? "bear"
          : "default"
      : "default";

  const trendKindRaw = (v2?.score_trend_kind ?? p.score_trend_kind)?.trim();
  const trendActionRaw = (v2?.score_trend_action ?? p.score_trend_action)?.trim();
  const trendDisplay = formatScoreTrendDisplay(t, trendKindRaw, trendActionRaw);
  const scenarioKindRaw = (v2?.position_scenario_kind ?? p.position_scenario_kind)?.trim();
  const scenarioDisplay = formatScenarioDisplay(t, scenarioKindRaw);
  const entryAtSetup = v2?.position_strength_entry_10 ?? p.position_strength_entry_10;
  const entryAtSetupStr =
    typeof entryAtSetup === "number" && Number.isFinite(entryAtSetup)
      ? `${entryAtSetup} / 10`
      : "—";

  const exhaustionDisp = formatDetectionPanel(t, te);
  const structureDisp = formatDetectionPanel(t, ss);
  const levelsOk = hasExecutableSignalSetupLevels(p, v2);

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

  const sep = (
    <tr key="_dashSep">
      <td colSpan={2} className="signal-dash-detail__sep-cell">
        <div className="signal-dash-detail__rule" role="presentation" />
      </td>
    </tr>
  );

  return (
    <>
      <div className="signal-dash-detail__banner">
        <div className="signal-dash-detail__banner-left mono">{displaySymbol}</div>
        <div className="signal-dash-detail__banner-right mono">{t("app.signalDashboardDrawer.strengthShort", { value: posStr })}</div>
      </div>
      <table className="signal-dash-detail__table">
        <tbody>
          {rk("status", pickDashboardStr(v2?.status, p.durum))}
          {rk("localTrend", formatTrendAxisPanel(t, v2?.local_trend, p.yerel_trend))}
          {rk("globalTrend", formatTrendAxisPanel(t, v2?.global_trend, p.global_trend))}
          {rk(
            "marketMode",
            formatMarketModePanel(t, v2?.market_mode, p.piyasa_modu, rsi14),
          )}
          {rk("entryMode", formatEntryModePanel(t, v2?.entry_mode, p.giris_modu))}
          {rk("volatilityPct", formatVolatilityPercentPanel(v2?.volatility_pct, p.oynaklik_pct))}
          {rk("momentum1", formatMomentumPanel(t, v2?.momentum_rsi, p.momentum_1))}
          {rk("momentum2", formatMomentumPanel(t, v2?.momentum_roc, p.momentum_2))}
          {rk("entryActual", pickDashboardNum(v2?.entry_price ?? undefined, p.giris_gercek ?? undefined))}
          {rk("stopInitial", pickDashboardNum(v2?.stop_initial ?? undefined, p.stop_ilk ?? undefined))}
          {rk(
            "takeProfitInitial",
            pickDashboardNum(v2?.take_profit_initial ?? undefined, p.kar_al_ilk ?? undefined),
          )}
          {sep}
          {rk(
            "stopTrailActive",
            levelsOk
              ? pickDashboardNum(v2?.stop_trail ?? undefined, p.stop_trail_aktif ?? undefined)
              : "—",
          )}
          {rk(
            "takeProfitDynamic",
            levelsOk
              ? pickDashboardNum(v2?.take_profit_dynamic ?? undefined, p.kar_al_dinamik ?? undefined)
              : "—",
          )}
          {rk("signalSource", formatTrendSignalSourcePanel(t), "accent")}
          {rk("trendExhaustion", exhaustionDisp.text, exhaustionDisp.toneKey)}
          {rk("structureShift", structureDisp.text, structureDisp.toneKey)}
          {rk("positionStrength", posStr, psTone)}
          {rk("scoreAtEntry", entryAtSetupStr)}
          {rk("scoreTrend", trendDisplay, scoreTrendToneFromKind(trendKindRaw))}
          {rk("positionScenario", scenarioDisplay, positionScenarioToneFromKind(scenarioKindRaw))}
          {rk("system", sysStr, sysTone)}
        </tbody>
      </table>
      <details className="signal-dash-detail__meta">
        <summary>{t("app.signalDashboard.detailMetaSummary")}</summary>
        <p className="muted signal-dash-detail__meta-note">{t("app.signalDashboard.priorityLine")}</p>
        <table className="signal-dash-detail__table signal-dash-detail__table--compact">
          <tbody>
            {rk("symbol", displaySymbol)}
            {rk("venueInterval", venueLine)}
            {rk("statusModelRaw", pickDashboardStr(v2?.status_model_raw, p.durum_model_raw))}
            {rk("directionPolicyDb", p.signal_direction_mode ?? "—")}
            {rk("directionEffective", p.signal_direction_effective ?? "—")}
            {rk("rangeWireSource", pickDashboardStr(v2?.signal_source, p.sinyal_kaynagi))}
            {rk("rangeHigh", formatDashboardNumber(p.range_high ?? undefined))}
            {rk("rangeLow", formatDashboardNumber(p.range_low ?? undefined))}
            {rk("rangeMid", formatDashboardNumber(p.range_mid ?? undefined))}
            {rk("atr", formatDashboardNumber(p.atr ?? undefined))}
            {rk("lastBar", p.last_bar_open_time ?? "—")}
            {rk("rsi14Last", rsi14 != null ? rsi14.toFixed(2) : "—")}
          </tbody>
        </table>
      </details>
      {v2 ? (
        <details className="signal-dash-detail__meta">
          <summary className="muted">
            {t("app.signalDashboard.wireSummary")} <code>signal_dashboard_v2</code>
          </summary>
          <table className="signal-dash-detail__table signal-dash-detail__table--compact mono muted">
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
              {wireRow("setup_entry_price", v2.setup_entry_price)}
              {wireRow("stop_initial", v2.stop_initial)}
              {wireRow("take_profit_initial", v2.take_profit_initial)}
              {wireRow("stop_trail", v2.stop_trail)}
              {wireRow("take_profit_dynamic", v2.take_profit_dynamic)}
              {wireRow("signal_source", v2.signal_source)}
              {wireRow("trend_exhaustion", v2.trend_exhaustion)}
              {wireRow("structure_shift", v2.structure_shift)}
              {wireRow("position_strength_10", v2.position_strength_10)}
              {wireRow("position_strength_history_10", v2.position_strength_history_10)}
              {wireRow("score_trend_kind", v2.score_trend_kind)}
              {wireRow("score_trend_action", v2.score_trend_action)}
              {wireRow("position_strength_entry_10", v2.position_strength_entry_10)}
              {wireRow("position_scenario_kind", v2.position_scenario_kind)}
              {wireRow("system_active", v2.system_active)}
              {wireRow("rsi_14_last", v2.rsi_14_last)}
            </tbody>
          </table>
        </details>
      ) : null}
    </>
  );
}

/**
 * Lists every `signal_dashboard` engine snapshot; row opens the detail layout (banner + key/value table).
 */
export function SignalDashboardDrawerPanel({ snapshots, chartMatchedEngineSymbolId }: Props) {
  const { t } = useTranslation();
  const [detailEngineSymbolId, setDetailEngineSymbolId] = useState<string | null>(null);

  useEffect(() => {
    if (detailEngineSymbolId && !snapshots.some((s) => s.engine_symbol_id === detailEngineSymbolId)) {
      setDetailEngineSymbolId(null);
    }
  }, [snapshots, detailEngineSymbolId]);

  const detailSnapshot = detailEngineSymbolId
    ? snapshots.find((s) => s.engine_symbol_id === detailEngineSymbolId) ?? null
    : null;

  if (snapshots.length === 0) {
    return (
      <p className="muted" style={{ fontSize: "0.75rem" }}>
        {t("app.signalDashboardDrawer.listEmpty")}
      </p>
    );
  }

  if (detailSnapshot) {
    return (
      <div className="signal-dash-detail">
        <button
          type="button"
          className="theme-toggle signal-dash-detail__back"
          onClick={() => setDetailEngineSymbolId(null)}
        >
          {t("app.signalDashboardDrawer.backToList")}
        </button>
        <SignalDashboardDetailBody snapshot={detailSnapshot} />
      </div>
    );
  }

  return (
    <div className="signal-dash-list-wrap">
      <p className="muted" style={{ fontSize: "0.68rem", margin: "0 0 0.4rem 0" }}>
        {t("app.signalDashboardDrawer.listHint")}
      </p>
      <div className="signal-dash-list" role="list">
        {snapshots.map((s) => {
          const accent = signalDashboardRowAccent(s);
          const { status, localTrend, positionStrength } = listRowSecondary(s, t);
          const sym = s.symbol?.trim() ? s.symbol.trim().toUpperCase() : "—";
          const venue = `${s.exchange?.trim() || "—"}/${s.segment?.trim() || "—"} · ${s.interval?.trim() || "—"}`;
          const isChart = chartMatchedEngineSymbolId != null && s.engine_symbol_id === chartMatchedEngineSymbolId;
          const statusTone = dashboardValueTone("status", status);
          const trendTone = dashboardValueTone("localTrend", localTrend);
          return (
            <button
              key={s.engine_symbol_id}
              type="button"
              role="listitem"
              className={`signal-dash-list__row ${accentClass(accent)}${isChart ? " signal-dash-list__row--chart" : ""}`}
              onClick={() => setDetailEngineSymbolId(s.engine_symbol_id)}
            >
              <div className="signal-dash-list__row-top">
                <span className="mono signal-dash-list__symbol">{sym}</span>
                {isChart ? (
                  <span className="signal-dash-list__chart-pill">{t("app.signalDashboardDrawer.chartBadge")}</span>
                ) : null}
              </div>
              <div className="mono muted signal-dash-list__venue">{venue}</div>
              <div className="signal-dash-list__row-meta">
                <span className={valueClassName(statusTone)}>{status}</span>
                <span className="muted"> · </span>
                <span className={valueClassName(trendTone)}>{localTrend}</span>
                <span className="muted"> · </span>
                <span className="mono muted">{positionStrength}</span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
