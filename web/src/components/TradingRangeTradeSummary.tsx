import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { RangeSetupFromEvents } from "../lib/rangeSetupsFromEvents";
import { setupPnlPct, setupPnlPctAfterFees } from "../lib/rangeSetupsFromEvents";

type Props = {
  setups: RangeSetupFromEvents[];
  takerFraction: number | null;
};

function maxAbsPnL(setups: RangeSetupFromEvents[], takerFraction: number | null): number {
  let max = 5;
  for (const row of setups) {
    const p = setupPnlPct(row);
    const n =
      takerFraction != null ? setupPnlPctAfterFees(row, takerFraction, takerFraction) : null;
    if (p != null && Number.isFinite(p)) max = Math.max(max, Math.abs(p));
    if (n != null && Number.isFinite(n)) max = Math.max(max, Math.abs(n));
  }
  return max;
}

function PnlMeter({
  label,
  valuePct,
  maxAbs,
}: {
  label: string;
  valuePct: number | null;
  maxAbs: number;
}) {
  if (valuePct == null || !Number.isFinite(valuePct)) {
    return (
      <div className="tv-tr-ts-meter">
        <div className="tv-tr-ts-meter-head">
          <span className="tv-tr-ts-meter-label">{label}</span>
          <span className="tv-tr-ts-meter-val tv-tr-ts-meter-val--na">—</span>
        </div>
        <div className="tv-tr-ts-meter-track tv-tr-ts-meter-track--empty" aria-hidden />
      </div>
    );
  }
  const ratio = Math.min(Math.abs(valuePct) / maxAbs, 1);
  const halfWidthPct = ratio * 50;
  const positive = valuePct >= 0;
  return (
    <div className="tv-tr-ts-meter">
      <div className="tv-tr-ts-meter-head">
        <span className="tv-tr-ts-meter-label">{label}</span>
        <span
          className={`tv-tr-ts-meter-val ${positive ? "tv-tr-ts-meter-val--pos" : "tv-tr-ts-meter-val--neg"}`}
        >
          {valuePct.toFixed(2)}%
        </span>
      </div>
      <div className="tv-tr-ts-meter-track" aria-hidden>
        <span className="tv-tr-ts-meter-axis" />
        {positive ? (
          <span
            className="tv-tr-ts-meter-fill tv-tr-ts-meter-fill--pos"
            style={{ width: `${halfWidthPct}%` }}
          />
        ) : (
          <span
            className="tv-tr-ts-meter-fill tv-tr-ts-meter-fill--neg"
            style={{ width: `${halfWidthPct}%` }}
          />
        )}
      </div>
    </div>
  );
}

export function TradingRangeTradeSummary({ setups, takerFraction }: Props) {
  const { t } = useTranslation();
  const maxAbs = useMemo(() => maxAbsPnL(setups, takerFraction), [setups, takerFraction]);

  if (setups.length === 0) {
    return (
      <p className="muted" style={{ fontSize: "0.72rem" }}>
        {t("app.tradingRangeSetup.empty")}
      </p>
    );
  }

  return (
    <div className="tv-tr-ts-root">
      <div className="tv-tr-ts-cards">
        {setups.map((row) => {
          const pnl = setupPnlPct(row);
          const pnlNet =
            takerFraction != null ? setupPnlPctAfterFees(row, takerFraction, takerFraction) : null;
          const entryPx =
            row.entry.reference_price != null && Number.isFinite(row.entry.reference_price)
              ? row.entry.reference_price.toFixed(4)
              : "—";
          const exitPx =
            row.exit?.reference_price != null && Number.isFinite(row.exit.reference_price)
              ? row.exit.reference_price.toFixed(4)
              : "—";
          const sideClass =
            row.side === "short" ? "tv-tr-ts-card--short" : "tv-tr-ts-card--long";
          const venueLine = `${row.exchange} · ${row.segment} · ${row.symbol} · ${row.interval}`;
          return (
            <article key={row.id} className={`tv-tr-ts-card ${sideClass}`}>
              <header className="tv-tr-ts-card-head">
                <span className="tv-tr-ts-side-badge">{row.side.toUpperCase()}</span>
                <span
                  className={
                    row.closed ? "tv-tr-ts-status tv-tr-ts-status--closed" : "tv-tr-ts-status tv-tr-ts-status--open"
                  }
                >
                  {row.closed
                    ? t("app.tradingRangeSetup.statusClosed")
                    : t("app.tradingRangeSetup.statusOpen")}
                </span>
              </header>
              <p className="mono muted" style={{ fontSize: "0.65rem", margin: "0.2rem 0 0.35rem", lineHeight: 1.35 }}>
                {venueLine}
              </p>
              <div className="tv-tr-ts-flow">
                <div className="tv-tr-ts-flow-col">
                  <span className="tv-tr-ts-flow-label">{t("app.tradingRangeSetup.colEntry")}</span>
                  <time className="tv-tr-ts-flow-time mono">{row.entry.bar_open_time}</time>
                  <span className="tv-tr-ts-flow-px mono">{entryPx}</span>
                </div>
                <div className="tv-tr-ts-flow-arrow" aria-hidden>
                  <svg width="20" height="20" viewBox="0 0 20 20" className="tv-tr-ts-flow-arrow-svg">
                    <path
                      d="M4 10h10M11 6l4 4-4 4"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.6"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                </div>
                <div className="tv-tr-ts-flow-col">
                  <span className="tv-tr-ts-flow-label">{t("app.tradingRangeSetup.colExit")}</span>
                  {row.exit ? (
                    <>
                      <time className="tv-tr-ts-flow-time mono">{row.exit.bar_open_time}</time>
                      <span className="tv-tr-ts-flow-px mono">{exitPx}</span>
                    </>
                  ) : (
                    <span className="tv-tr-ts-flow-na muted">—</span>
                  )}
                </div>
              </div>
              <div className="tv-tr-ts-meters">
                <PnlMeter label={t("app.tradingRangeSetup.colPnlPct")} valuePct={pnl} maxAbs={maxAbs} />
                <PnlMeter label={t("app.tradingRangeSetup.colPnlNet")} valuePct={pnlNet} maxAbs={maxAbs} />
              </div>
            </article>
          );
        })}
      </div>
      <details className="tv-tr-ts-details">
        <summary className="tv-tr-ts-details-summary">{t("app.tradingRangeSetup.tableDetailsSummary")}</summary>
        <div className="tv-tr-ts-details-inner">
          <table
            className="mono muted tv-tr-ts-table"
            style={{
              width: "100%",
              fontSize: "0.68rem",
              borderCollapse: "collapse",
              marginTop: "0.35rem",
            }}
          >
            <thead>
              <tr>
                <th
                  className="muted"
                  style={{ textAlign: "left", padding: "0.2rem 0.4rem 0.2rem 0", fontWeight: 600 }}
                >
                  {t("app.tradingRangeSetup.colSide")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colExchange")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colMarket")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colSymbol")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colTf")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colEntry")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colExit")}
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.2rem 0.35rem", fontWeight: 600 }}>
                  {t("app.tradingRangeSetup.colStatus")}
                </th>
                <th
                  className="muted"
                  style={{ textAlign: "right", padding: "0.2rem 0 0.2rem 0.35rem", fontWeight: 600 }}
                >
                  {t("app.tradingRangeSetup.colPnlPct")}
                </th>
                <th
                  className="muted"
                  style={{ textAlign: "right", padding: "0.2rem 0 0.2rem 0.35rem", fontWeight: 600 }}
                >
                  {t("app.tradingRangeSetup.colPnlNet")}
                </th>
              </tr>
            </thead>
            <tbody>
              {setups.map((row) => {
                const pnl = setupPnlPct(row);
                const pnlNet =
                  takerFraction != null
                    ? setupPnlPctAfterFees(row, takerFraction, takerFraction)
                    : null;
                const entryPx =
                  row.entry.reference_price != null && Number.isFinite(row.entry.reference_price)
                    ? row.entry.reference_price.toFixed(4)
                    : "—";
                const exitPx =
                  row.exit?.reference_price != null && Number.isFinite(row.exit.reference_price)
                    ? row.exit.reference_price.toFixed(4)
                    : "—";
                return (
                  <tr key={`tbl-${row.id}`}>
                    <td style={{ padding: "0.18rem 0.4rem 0.18rem 0", whiteSpace: "nowrap" }}>
                      {row.side.toUpperCase()}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", whiteSpace: "nowrap" }}>
                      {row.exchange}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", whiteSpace: "nowrap" }}>
                      {row.segment}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", whiteSpace: "nowrap" }}>
                      {row.symbol}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", whiteSpace: "nowrap" }}>
                      {row.interval}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", wordBreak: "break-all" }}>
                      {row.entry.bar_open_time}
                      <br />
                      <span style={{ opacity: 0.9 }}>{entryPx}</span>
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", wordBreak: "break-all" }}>
                      {row.exit ? (
                        <>
                          {row.exit.bar_open_time}
                          <br />
                          <span style={{ opacity: 0.9 }}>{exitPx}</span>
                        </>
                      ) : (
                        "—"
                      )}
                    </td>
                    <td style={{ padding: "0.18rem 0.35rem", whiteSpace: "nowrap" }}>
                      {row.closed
                        ? t("app.tradingRangeSetup.statusClosed")
                        : t("app.tradingRangeSetup.statusOpen")}
                    </td>
                    <td style={{ padding: "0.18rem 0 0.18rem 0.35rem", textAlign: "right" }}>
                      {pnl != null && Number.isFinite(pnl) ? `${pnl.toFixed(2)}%` : "—"}
                    </td>
                    <td style={{ padding: "0.18rem 0 0.18rem 0.35rem", textAlign: "right" }}>
                      {pnlNet != null && Number.isFinite(pnlNet) ? `${pnlNet.toFixed(2)}%` : "—"}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </details>
    </div>
  );
}
