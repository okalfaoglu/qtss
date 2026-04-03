import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { EngineSnapshotJoinedApiRow, EngineSymbolApiRow } from "../api/client";
import type { EngineTargetLookup } from "../lib/engineTargetMatch";
import {
  classifyTradingRangeSnapshotRow,
  engineSymbolMatchesToolbarVenue,
  findTradingRangeSnapshotForTarget,
} from "../lib/tradingRangeSnapshotListHelpers";
import { TradingRangeEngineEvalDialog } from "./TradingRangeEngineEvalDialog";

type Props = {
  engineSymbols: EngineSymbolApiRow[];
  engineSnapshots: EngineSnapshotJoinedApiRow[];
  toolbarExchange: string;
  toolbarSegment: string;
};

function targetFromSymbolRow(row: EngineSymbolApiRow): EngineTargetLookup {
  return {
    exchange: row.exchange,
    segment: row.segment,
    symbol: row.symbol,
    interval: row.interval,
  };
}

export function TradingRangeSetupEngineSymbolsPanel({
  engineSymbols,
  engineSnapshots,
  toolbarExchange,
  toolbarSegment,
}: Props) {
  const { t } = useTranslation();
  const [evalTarget, setEvalTarget] = useState<EngineTargetLookup | null>(null);

  const rows = useMemo(() => {
    return engineSymbols
      .filter((r) => engineSymbolMatchesToolbarVenue(r, toolbarExchange, toolbarSegment))
      .slice()
      .sort((a, b) => {
        if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order;
        const sym = a.symbol.localeCompare(b.symbol);
        if (sym !== 0) return sym;
        return a.interval.localeCompare(b.interval);
      });
  }, [engineSymbols, toolbarExchange, toolbarSegment]);

  return (
    <div className="tv-tr-eng-setup">
      <TradingRangeEngineEvalDialog
        open={evalTarget != null}
        target={evalTarget}
        engineSnapshots={engineSnapshots}
        engineSymbols={engineSymbols}
        onClose={() => setEvalTarget(null)}
      />
      <p className="muted" style={{ fontSize: "0.66rem", margin: "0 0 0.45rem 0", lineHeight: 1.45 }}>
        {t("app.tradingRangeEventsSetup.engineListIntro")}
      </p>
      {rows.length === 0 ? (
        <p className="muted" style={{ fontSize: "0.72rem", margin: 0 }}>
          {t("app.tradingRangeEventsSetup.engineListEmptyVenue")}
        </p>
      ) : (
        <div className="tv-tr-eng-list" role="list">
          {rows.map((row) => {
            const target = targetFromSymbolRow(row);
            const trSnap = findTradingRangeSnapshotForTarget(engineSnapshots, target);
            const cl = classifyTradingRangeSnapshotRow(trSnap);
            let rowClass = "tv-tr-eng-list__row tv-tr-eng-list__row--muted";
            let summary = "";
            switch (cl.kind) {
              case "no_snapshot":
                summary = t("app.tradingRangeEventsSetup.trRowNoSnapshot");
                break;
              case "error": {
                rowClass = "tv-tr-eng-list__row tv-tr-eng-list__row--error";
                const msg = cl.errorMessage ?? "—";
                summary = t("app.tradingRangeEventsSetup.trRowError", {
                  message: msg.length > 96 ? `${msg.slice(0, 96)}…` : msg,
                });
                break;
              }
              case "insufficient_bars":
                summary = t("app.tradingRangeEventsSetup.trRowInsufficient");
                break;
              case "empty_payload":
                summary = t("app.tradingRangeEventsSetup.trRowEmptyPayload");
                break;
              case "ok": {
                rowClass = "tv-tr-eng-list__row tv-tr-eng-list__row--ok";
                const p = cl.payload!;
                const side = String(p.setup_side ?? "—");
                const score =
                  typeof p.setup_score_best === "number" && Number.isFinite(p.setup_score_best)
                    ? String(p.setup_score_best)
                    : "—";
                const validStr =
                  p.valid === true
                    ? t("app.tradingRangeEngineEval.yes")
                    : p.valid === false
                      ? t("app.tradingRangeEngineEval.no")
                      : "—";
                summary = t("app.tradingRangeEventsSetup.trRowOkSummary", {
                  side,
                  score,
                  valid: validStr,
                });
                break;
              }
            }
            const symU = row.symbol.trim().toUpperCase();
            const disabled = !row.enabled;
            return (
              <button
                key={row.id}
                type="button"
                role="listitem"
                disabled={disabled}
                title={
                  disabled
                    ? t("app.tradingRangeEventsSetup.engineRowDisabledHint")
                    : t("app.tradingRangeEventsSetup.engineRowOpenHint")
                }
                className={`${rowClass}${disabled ? " tv-tr-eng-list__row--disabled" : ""}`}
                onClick={() => {
                  if (!disabled) setEvalTarget(target);
                }}
              >
                <div className="tv-tr-eng-list__row-top">
                  <span className="mono tv-tr-eng-list__sym">{symU}</span>
                  <span className="mono muted tv-tr-eng-list__iv">{row.interval}</span>
                  {!row.enabled ? (
                    <span className="tv-tr-eng-list__pill-off">{t("app.tradingRangeEventsSetup.badgeDisabled")}</span>
                  ) : null}
                </div>
                <div className="muted mono tv-tr-eng-list__meta" style={{ fontSize: "0.65rem" }}>
                  {row.label?.trim() || "—"}
                </div>
                <div className="tv-tr-eng-list__summary">{summary}</div>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
