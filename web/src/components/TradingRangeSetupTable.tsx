import { useTranslation } from "react-i18next";
import type { RangeSignalEventApiRow } from "../api/client";
import { payloadOptionalNumber } from "../lib/parseRangeSignalPayload";

type Props = {
  events: RangeSignalEventApiRow[];
};

function formatPx(n: number | null): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return n.toFixed(4);
}

function eventBadgeClass(kind: string): string {
  if (kind === "long_entry") return "tv-tr-setup-badge tv-tr-setup-badge--long-entry";
  if (kind === "long_exit") return "tv-tr-setup-badge tv-tr-setup-badge--long-exit";
  if (kind === "short_entry") return "tv-tr-setup-badge tv-tr-setup-badge--short-entry";
  if (kind === "short_exit") return "tv-tr-setup-badge tv-tr-setup-badge--short-exit";
  return "tv-tr-setup-badge tv-tr-setup-badge--other";
}

export function TradingRangeSetupTable({ events }: Props) {
  const { t } = useTranslation();

  if (events.length === 0) {
    return (
      <p className="muted tv-tr-setup-empty" style={{ fontSize: "0.72rem", margin: 0 }}>
        {t("app.tradingRangeEventsSetup.empty")}
      </p>
    );
  }

  return (
    <div className="tv-tr-setup-wrap">
      <table className="tv-tr-setup-table">
        <thead>
          <tr>
            <th scope="col">{t("app.tradingRangeEventsSetup.colEvent")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colExchange")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colMarket")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colSymbol")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colTf")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colEnter")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colStop")}</th>
            <th scope="col">{t("app.tradingRangeEventsSetup.colTp")}</th>
          </tr>
        </thead>
        <tbody>
          {events.map((ev) => {
            const enter =
              ev.reference_price != null && Number.isFinite(ev.reference_price)
                ? ev.reference_price
                : payloadOptionalNumber(ev.payload, "giris_gercek");
            const stop = payloadOptionalNumber(ev.payload, "stop_ilk");
            const tp = payloadOptionalNumber(ev.payload, "kar_al_ilk");
            return (
              <tr key={ev.id} className="tv-tr-setup-row">
                <td>
                  <span className={eventBadgeClass(ev.event_kind)}>{ev.event_kind}</span>
                </td>
                <td className="tv-tr-setup-mono">{ev.exchange}</td>
                <td className="tv-tr-setup-mono">{ev.segment}</td>
                <td className="tv-tr-setup-mono tv-tr-setup-symbol">{ev.symbol}</td>
                <td className="tv-tr-setup-mono">{ev.interval}</td>
                <td className="tv-tr-setup-enter">
                  <span className="tv-tr-setup-enter-px">{formatPx(enter)}</span>
                  <span className="tv-tr-setup-enter-bar">
                    {t("app.tradingRangeEventsSetup.barLabel")} {ev.bar_open_time}
                  </span>
                </td>
                <td className="tv-tr-setup-mono tv-tr-setup-num">{formatPx(stop)}</td>
                <td className="tv-tr-setup-mono tv-tr-setup-num">{formatPx(tp)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
