import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { EngineSnapshotJoinedApiRow, EngineSymbolApiRow } from "../api/client";
import {
  engineRowMatchesTarget,
  normalizeEngineMarketSegment,
  type EngineTargetLookup,
} from "../lib/engineTargetMatch";
import {
  parseSignalDashboardV2,
  pickDashboardStr,
  trendAxisDisplayAsLongShort,
  type SignalDashboardPayload,
} from "../lib/signalDashboardPayload";
import type { TradingRangeDbPayload } from "../lib/tradingRangeDbOverlay";

type Props = {
  open: boolean;
  target: EngineTargetLookup | null;
  engineSnapshots: EngineSnapshotJoinedApiRow[];
  engineSymbols: EngineSymbolApiRow[];
  onClose: () => void;
};

function fmtNum(n: unknown): string {
  if (n == null || typeof n !== "number" || !Number.isFinite(n)) return "—";
  return String(n);
}

function fmtBool(v: unknown): string {
  if (v === true) return "true";
  if (v === false) return "false";
  return "—";
}

export function TradingRangeEngineEvalDialog({
  open,
  target,
  engineSnapshots,
  engineSymbols,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open) {
      if (!el.open) el.showModal();
    } else if (el.open) {
      el.close();
    }
  }, [open]);

  const motorRow = useMemo(() => {
    if (!target) return null;
    return engineSymbols.find((r) => engineRowMatchesTarget(r, target)) ?? null;
  }, [engineSymbols, target]);

  const trSnap = useMemo(() => {
    if (!target) return null;
    return (
      engineSnapshots.find((s) => s.engine_kind === "trading_range" && engineRowMatchesTarget(s, target)) ?? null
    );
  }, [engineSnapshots, target]);

  const dashSnap = useMemo(() => {
    if (!target) return null;
    return (
      engineSnapshots.find((s) => s.engine_kind === "signal_dashboard" && engineRowMatchesTarget(s, target)) ?? null
    );
  }, [engineSnapshots, target]);

  const trPayload = trSnap?.payload && typeof trSnap.payload === "object" ? (trSnap.payload as TradingRangeDbPayload) : null;

  const dashSummary = useMemo(() => {
    if (!dashSnap?.payload || typeof dashSnap.payload !== "object") return null;
    const raw = dashSnap.payload as Record<string, unknown>;
    if (raw.reason === "insufficient_bars") return { insufficient: true as const };
    const p = dashSnap.payload as SignalDashboardPayload;
    const v2 = parseSignalDashboardV2(raw.signal_dashboard_v2);
    return {
      insufficient: false as const,
      status: pickDashboardStr(v2?.status, p.durum),
      localTrend: trendAxisDisplayAsLongShort(pickDashboardStr(v2?.local_trend, p.yerel_trend)),
      globalTrend: trendAxisDisplayAsLongShort(pickDashboardStr(v2?.global_trend, p.global_trend)),
      marketMode: pickDashboardStr(v2?.market_mode, p.piyasa_modu),
      pos:
        v2?.position_strength_10 != null
          ? `${v2.position_strength_10} / 10`
          : p.pozisyon_gucu_10 != null
            ? `${p.pozisyon_gucu_10} / 10`
            : "—",
    };
  }, [dashSnap]);

  if (!target) return null;

  const venue = `${target.exchange}/${normalizeEngineMarketSegment(target.segment)} · ${target.interval}`;
  const symU = target.symbol.trim().toUpperCase();

  return (
    <dialog
      ref={ref}
      className="tv-engine-eval-dialog"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="tv-engine-eval-dialog__panel" onClick={(e) => e.stopPropagation()}>
        <header className="tv-engine-eval-dialog__head">
          <div>
            <p className="tv-engine-eval-dialog__title">{t("app.tradingRangeEngineEval.title", { symbol: symU })}</p>
            <p className="mono muted tv-engine-eval-dialog__venue">{venue}</p>
          </div>
          <button type="button" className="theme-toggle tv-engine-eval-dialog__close" onClick={onClose}>
            {t("app.tradingRangeEngineEval.close")}
          </button>
        </header>

        <section className="tv-engine-eval-dialog__section">
          <h3 className="tv-engine-eval-dialog__h">{t("app.tradingRangeEngineEval.sectionMotor")}</h3>
          {motorRow ? (
            <dl className="tv-engine-eval-dialog__dl">
              <dt className="muted">{t("app.tradingRangeEngineEval.colLabel")}</dt>
              <dd className="mono">{motorRow.label?.trim() || "—"}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.colEnabled")}</dt>
              <dd className="mono">{motorRow.enabled ? t("app.tradingRangeEngineEval.yes") : t("app.tradingRangeEngineEval.no")}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.colDirection")}</dt>
              <dd className="mono">{motorRow.signal_direction_mode ?? "—"}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.colSort")}</dt>
              <dd className="mono">{motorRow.sort_order}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.colId")}</dt>
              <dd className="mono" style={{ wordBreak: "break-all", fontSize: "0.65rem" }}>
                {motorRow.id}
              </dd>
            </dl>
          ) : (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.noMotorRow")}</p>
          )}
        </section>

        <section className="tv-engine-eval-dialog__section">
          <h3 className="tv-engine-eval-dialog__h">{t("app.tradingRangeEngineEval.sectionTradingRange")}</h3>
          {!trSnap ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.noTradingRange")}</p>
          ) : trSnap.error ? (
            <p className="err tv-engine-eval-dialog__empty">{trSnap.error}</p>
          ) : !trPayload ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.payloadEmpty")}</p>
          ) : trPayload.reason === "insufficient_bars" ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.insufficientBars")}</p>
          ) : (
            <dl className="tv-engine-eval-dialog__dl">
              <dt className="muted">{t("app.tradingRangeEngineEval.trValid")}</dt>
              <dd className="mono">{fmtBool(trPayload.valid)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trRangeRegime")}</dt>
              <dd className="mono">{fmtBool(trPayload.is_range_regime)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trSetupSide")}</dt>
              <dd className="mono">{String(trPayload.setup_side ?? "—")}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trScoreBest")}</dt>
              <dd className="mono">{fmtNum(trPayload.setup_score_best)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trGuardrails")}</dt>
              <dd className="mono">
                {trPayload.guardrails_pass === true
                  ? "PASS"
                  : trPayload.guardrails_pass === false
                    ? "REJECT"
                    : "—"}
              </dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trZone")}</dt>
              <dd className="mono">{String(trPayload.range_zone ?? "—")}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trRangeHigh")}</dt>
              <dd className="mono">{fmtNum(trPayload.range_high)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trRangeLow")}</dt>
              <dd className="mono">{fmtNum(trPayload.range_low)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trMid")}</dt>
              <dd className="mono">{fmtNum(trPayload.mid)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trTouches")}</dt>
              <dd className="mono">
                {fmtNum(trPayload.support_touches)}/{fmtNum(trPayload.resistance_touches)}
              </dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trCloseBreakout")}</dt>
              <dd className="mono">{fmtBool(trPayload.close_breakout)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trScoreLong")}</dt>
              <dd className="mono">{fmtNum(trPayload.setup_score_long)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.trScoreShort")}</dt>
              <dd className="mono">{fmtNum(trPayload.setup_score_short)}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.snapBarCount")}</dt>
              <dd className="mono">{trSnap.bar_count ?? "—"}</dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.snapLastBar")}</dt>
              <dd className="mono" style={{ wordBreak: "break-all" }}>
                {trSnap.last_bar_open_time ?? trPayload.last_bar_open_time ?? "—"}
              </dd>
              <dt className="muted">{t("app.tradingRangeEngineEval.snapComputed")}</dt>
              <dd className="mono" style={{ wordBreak: "break-all" }}>
                {trSnap.computed_at}
              </dd>
            </dl>
          )}
        </section>

        <section className="tv-engine-eval-dialog__section">
          <h3 className="tv-engine-eval-dialog__h">{t("app.tradingRangeEngineEval.sectionSignal")}</h3>
          {!dashSnap ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.noSignalDashboard")}</p>
          ) : dashSnap.error ? (
            <p className="err tv-engine-eval-dialog__empty">{dashSnap.error}</p>
          ) : !dashSummary ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.payloadEmpty")}</p>
          ) : dashSummary.insufficient ? (
            <p className="muted tv-engine-eval-dialog__empty">{t("app.tradingRangeEngineEval.insufficientBars")}</p>
          ) : (
            <>
              <dl className="tv-engine-eval-dialog__dl">
                <dt className="muted">{t("app.signalDashboard.row.status")}</dt>
                <dd className="mono">{dashSummary.status}</dd>
                <dt className="muted">{t("app.signalDashboard.row.localTrend")}</dt>
                <dd className="mono">{dashSummary.localTrend}</dd>
                <dt className="muted">{t("app.signalDashboard.row.globalTrend")}</dt>
                <dd className="mono">{dashSummary.globalTrend}</dd>
                <dt className="muted">{t("app.signalDashboard.row.marketMode")}</dt>
                <dd className="mono">{dashSummary.marketMode}</dd>
                <dt className="muted">{t("app.signalDashboard.row.positionStrength")}</dt>
                <dd className="mono">{dashSummary.pos}</dd>
                <dt className="muted">{t("app.tradingRangeEngineEval.snapComputed")}</dt>
                <dd className="mono" style={{ wordBreak: "break-all" }}>
                  {dashSnap.computed_at}
                </dd>
              </dl>
              <p className="muted" style={{ fontSize: "0.62rem", margin: "0.35rem 0 0", lineHeight: 1.4 }}>
                {t("app.tradingRangeEngineEval.signalTabHint")}
              </p>
            </>
          )}
        </section>
      </div>
    </dialog>
  );
}
