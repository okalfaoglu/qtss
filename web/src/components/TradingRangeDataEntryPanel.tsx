import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  patchEngineSymbol,
  postEngineSymbolsBulk,
  type EngineSymbolApiRow,
  type EngineSymbolsBulkTarget,
} from "../api/client";

function parseEngineSymbolsBulkLines(text: string): EngineSymbolsBulkTarget[] {
  const out: EngineSymbolsBulkTarget[] = [];
  for (const rawLine of text.split(/\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const parts = line.split(/[\s,]+/).filter(Boolean);
    if (parts.length === 2) {
      out.push({
        symbol: parts[0].toUpperCase(),
        interval: parts[1],
      });
    } else if (parts.length >= 4) {
      out.push({
        exchange: parts[0].toLowerCase(),
        segment: parts[1].toLowerCase(),
        symbol: parts[2].toUpperCase(),
        interval: parts[3],
      });
    }
  }
  return out;
}

export type TradingRangeDataEntryPanelProps = {
  accessToken: string | null;
  onEnginesUpdated?: () => void | Promise<void>;
  isLoggedIn: boolean;
  canOps: boolean;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  limit: string;
  onExchangeChange: (value: string) => void;
  onSegmentChange: (value: string) => void;
  onSymbolChange: (value: string) => void;
  onIntervalChange: (value: string) => void;
  onLimitChange: (value: string) => void;
  onApplyChart: () => void;
  applyChartBusy: boolean;
  onBackfillRest?: () => void;
  backfillDisabled: boolean;
  backfillBusy: boolean;
  backfillNote: string;
  engineSymbolDraft: string;
  engineIntervalDraft: string;
  onEngineSymbolDraftChange: (value: string) => void;
  onEngineIntervalDraftChange: (value: string) => void;
  onSyncScopeToEngineDraft: () => void;
  onRegisterEngine: () => void | Promise<void>;
  engineRegisterBusy: boolean;
  registeredTargets: EngineSymbolApiRow[];
  onEngineTargetsPatchError?: (message: string) => void;
};

export function TradingRangeDataEntryPanel(props: TradingRangeDataEntryPanelProps) {
  const { t } = useTranslation();
  const {
    accessToken,
    onEnginesUpdated,
    isLoggedIn,
    canOps,
    exchange,
    segment,
    symbol,
    interval,
    limit,
    onExchangeChange,
    onSegmentChange,
    onSymbolChange,
    onIntervalChange,
    onLimitChange,
    onApplyChart,
    applyChartBusy,
    onBackfillRest,
    backfillDisabled,
    backfillBusy,
    backfillNote,
    engineSymbolDraft,
    engineIntervalDraft,
    onEngineSymbolDraftChange,
    onEngineIntervalDraftChange,
    onSyncScopeToEngineDraft,
    onRegisterEngine,
    engineRegisterBusy,
    registeredTargets,
    onEngineTargetsPatchError,
  } = props;

  const [bulkText, setBulkText] = useState("");
  const [bulkBusy, setBulkBusy] = useState(false);
  const [bulkSummary, setBulkSummary] = useState<string | null>(null);

  return (
    <>
      <p className="tv-drawer__section-head">{t("app.tradingRangeDrawer.dataEntryTitle")}</p>
      <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
        {t("app.tradingRangeDrawer.dataEntryIntro")}
      </p>
      {!isLoggedIn ? (
        <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.45rem" }}>
          {t("app.tradingRangeDrawer.dataEntryLoginHint")}
        </p>
      ) : null}
      <div className="tv-settings__fields">
        <label>
          <span className="muted">exchange</span>
          <input className="mono" value={exchange} onChange={(e) => onExchangeChange(e.target.value)} />
        </label>
        <label>
          <span className="muted">segment</span>
          <input className="mono" value={segment} onChange={(e) => onSegmentChange(e.target.value)} />
        </label>
        <label>
          <span className="muted">symbol</span>
          <input className="mono" value={symbol} onChange={(e) => onSymbolChange(e.target.value)} placeholder="BTCUSDT" />
        </label>
        <label>
          <span className="muted">interval</span>
          <input className="mono" value={interval} onChange={(e) => onIntervalChange(e.target.value)} placeholder="15m" />
        </label>
        <label>
          <span className="muted">limit</span>
          <input className="mono" value={limit} onChange={(e) => onLimitChange(e.target.value)} />
        </label>
      </div>
      <div style={{ marginTop: "0.45rem", display: "flex", flexWrap: "wrap", gap: "0.35rem", alignItems: "center" }}>
        <button
          type="button"
          className="theme-toggle"
          style={{ fontSize: "0.78rem" }}
          disabled={applyChartBusy}
          onClick={() => onApplyChart()}
        >
          {applyChartBusy ? t("app.tradingRangeDrawer.applyChartBusy") : t("app.tradingRangeDrawer.applyChart")}
        </button>
      </div>
      {isLoggedIn && canOps && onBackfillRest ? (
        <div style={{ marginTop: "0.55rem" }}>
          <p className="muted" style={{ fontSize: "0.7rem", marginBottom: "0.35rem", lineHeight: 1.45 }}>
            {t("app.tradingRangeDrawer.backfillRestHint")}
          </p>
          <button
            type="button"
            className="theme-toggle"
            style={{ fontSize: "0.78rem" }}
            onClick={() => onBackfillRest()}
            disabled={backfillDisabled}
          >
            {backfillBusy ? t("app.tradingRangeDrawer.backfillRestBusy") : t("app.tradingRangeDrawer.backfillRest")}
          </button>
          {backfillNote ? (
            <p className="muted" style={{ fontSize: "0.72rem", marginTop: "0.35rem" }}>
              {backfillNote}
            </p>
          ) : null}
        </div>
      ) : isLoggedIn && !canOps ? (
        <p className="muted" style={{ fontSize: "0.68rem", marginTop: "0.45rem", lineHeight: 1.45 }}>
          {t("app.tradingRangeDrawer.backfillRbacHint")}
        </p>
      ) : null}
      <p className="tv-drawer__section-head" style={{ marginTop: "0.65rem", marginBottom: "0.25rem" }}>
        {t("app.tradingRangeDrawer.engineTargetSubhead")}
      </p>
      <p className="muted" style={{ fontSize: "0.7rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
        {t("app.tradingRangeDrawer.engineTargetHint")}
      </p>
      <button
        type="button"
        className="theme-toggle"
        style={{ fontSize: "0.72rem", marginBottom: "0.35rem" }}
        onClick={() => onSyncScopeToEngineDraft()}
      >
        {t("app.tradingRangeDrawer.syncScopeToEngine")}
      </button>
      <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.35rem" }}>
        {t("app.engineDrawer.addTargetLead")}
        {!canOps ? (
          <span>
            {" "}
            {t("app.engineDrawer.addTargetRbacFull")}
          </span>
        ) : null}
      </p>
      {canOps ? (
        <>
          <div className="tv-settings__fields" style={{ marginTop: "0.2rem" }}>
            <label>
              <span className="muted">symbol</span>
              <input
                className="mono"
                value={engineSymbolDraft}
                onChange={(e) => onEngineSymbolDraftChange(e.target.value)}
                placeholder="BTCUSDT"
              />
            </label>
            <label>
              <span className="muted">interval</span>
              <input
                className="mono"
                value={engineIntervalDraft}
                onChange={(e) => onEngineIntervalDraftChange(e.target.value)}
                placeholder="4h"
              />
            </label>
          </div>
          <button
            type="button"
            className="theme-toggle"
            style={{ marginTop: "0.4rem", fontSize: "0.78rem" }}
            disabled={engineRegisterBusy}
            onClick={() => void onRegisterEngine()}
          >
            {engineRegisterBusy
              ? t("app.tradingRangeDrawer.addToEngineSymbolsBusy")
              : t("app.tradingRangeDrawer.addToEngineSymbols")}
          </button>
          <p className="tv-drawer__section-head" style={{ marginTop: "0.65rem", marginBottom: "0.25rem" }}>
            {t("app.tradingRangeDrawer.bulkTargetsTitle")}
          </p>
          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.35rem", lineHeight: 1.45 }}>
            {t("app.tradingRangeDrawer.bulkTargetsIntro")}
          </p>
          <textarea
            className="mono"
            rows={6}
            style={{
              width: "100%",
              fontSize: "0.72rem",
              padding: "0.35rem 0.45rem",
              borderRadius: "8px",
              border: "1px solid var(--tv-border, rgba(255,255,255,0.12))",
              background: "var(--card)",
              color: "var(--fg)",
              resize: "vertical",
            }}
            placeholder={t("app.tradingRangeDrawer.bulkTargetsPlaceholder")}
            value={bulkText}
            onChange={(e) => {
              setBulkText(e.target.value);
              setBulkSummary(null);
            }}
          />
          <button
            type="button"
            className="theme-toggle"
            style={{ marginTop: "0.4rem", fontSize: "0.78rem" }}
            disabled={bulkBusy || !accessToken}
            onClick={async () => {
              if (!accessToken) return;
              const targets = parseEngineSymbolsBulkLines(bulkText);
              if (targets.length === 0) {
                setBulkSummary(t("app.tradingRangeDrawer.bulkEmpty"));
                return;
              }
              setBulkBusy(true);
              setBulkSummary(null);
              try {
                const res = await postEngineSymbolsBulk(accessToken, {
                  exchange: exchange.trim() || undefined,
                  segment: segment.trim() || undefined,
                  targets,
                });
                setBulkSummary(
                  t("app.tradingRangeDrawer.bulkResult", {
                    inserted: res.inserted.length,
                    errors: res.errors.length,
                  }),
                );
                if (res.inserted.length > 0) await onEnginesUpdated?.();
              } catch (e) {
                setBulkSummary(String(e));
              } finally {
                setBulkBusy(false);
              }
            }}
          >
            {bulkBusy ? t("app.tradingRangeDrawer.bulkBusy") : t("app.tradingRangeDrawer.bulkSubmit")}
          </button>
          {bulkSummary ? (
            <p className="muted" style={{ fontSize: "0.7rem", marginTop: "0.35rem", whiteSpace: "pre-wrap" }}>
              {bulkSummary}
            </p>
          ) : null}
        </>
      ) : null}
      {isLoggedIn ? (
        <>
          <p className="tv-drawer__section-head" style={{ marginTop: "0.65rem", marginBottom: "0.25rem" }}>
            {t("app.tradingRangeDrawer.registeredTargetsHead", { count: registeredTargets.length })}
          </p>
          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.35rem", lineHeight: 1.45 }}>
            {t("app.tradingRangeDrawer.registeredTargetsHint")}
          </p>
          {registeredTargets.length === 0 ? (
            <p className="muted" style={{ fontSize: "0.72rem" }}>
              {t("app.tradingRangeDrawer.registeredTargetsEmpty")}
            </p>
          ) : (
            <ul className="tv-drawer-target-list muted mono">
              {registeredTargets.map((s) => (
                <li key={s.id} className="tv-drawer-target-list__item">
                  <span className="tv-drawer-target-list__meta">
                    {s.enabled ? "●" : "○"} {s.exchange}/{s.segment} {s.symbol} {s.interval}
                    {s.label ? ` — ${s.label}` : ""}
                  </span>
                  {canOps && accessToken ? (
                    <div className="tv-drawer-target-list__actions">
                      <select
                        className="mono"
                        value={(s.signal_direction_mode ?? "auto_segment").toLowerCase()}
                        title={t("app.engineDrawer.signalModeTitle")}
                        onChange={async (e) => {
                          try {
                            await patchEngineSymbol(accessToken, s.id, {
                              signal_direction_mode: e.target.value,
                            });
                            await onEnginesUpdated?.();
                          } catch (err) {
                            onEngineTargetsPatchError?.(String(err));
                          }
                        }}
                      >
                        <option value="auto_segment">{t("app.engineDrawer.optAutoSegment")}</option>
                        <option value="long_only">{t("app.engineDrawer.optLongOnly")}</option>
                        <option value="both">{t("app.engineDrawer.optBoth")}</option>
                        <option value="short_only">{t("app.engineDrawer.optShortOnly")}</option>
                      </select>
                      <button
                        type="button"
                        className="theme-toggle"
                        onClick={async () => {
                          try {
                            await patchEngineSymbol(accessToken, s.id, { enabled: !s.enabled });
                            await onEnginesUpdated?.();
                          } catch (e) {
                            onEngineTargetsPatchError?.(String(e));
                          }
                        }}
                      >
                        {s.enabled ? t("app.engineDrawer.toggleDisable") : t("app.engineDrawer.toggleEnable")}
                      </button>
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </>
      ) : null}
    </>
  );
}
