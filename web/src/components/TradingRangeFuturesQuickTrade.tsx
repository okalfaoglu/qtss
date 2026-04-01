import { useState } from "react";
import { useTranslation } from "react-i18next";
import { postMyBinancePlaceOrder } from "../api/client";

type Props = {
  accessToken: string;
  exchange: string;
  segment: string;
  symbol: string;
};

function normalizeSegmentForFutures(s: string): string {
  const x = s.trim().toLowerCase();
  if (x === "futures" || x === "usdt_futures" || x === "fapi") return "futures";
  return x || "spot";
}

export function TradingRangeFuturesQuickTrade({ accessToken, exchange, segment, symbol }: Props) {
  const { t } = useTranslation();
  const ex = exchange.trim().toLowerCase();
  const seg = normalizeSegmentForFutures(segment);
  const sym = symbol.trim().toUpperCase();
  const [qty, setQty] = useState("0.001");
  const [positionSide, setPositionSide] = useState<"" | "LONG" | "SHORT">("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  if (ex !== "binance" || seg !== "futures" || !sym) return null;

  const runMarket = async (side: "buy" | "sell", reduceOnly: boolean) => {
    setErr("");
    const qn = Number(qty);
    if (!Number.isFinite(qn) || qn <= 0) {
      setErr(t("app.tradingRangeFutures.invalidQty"));
      return;
    }
    setBusy(true);
    try {
      await postMyBinancePlaceOrder(accessToken, {
        instrument: { exchange: "binance", segment: "futures", symbol: sym },
        side,
        quantity: String(qn),
        order_type: { type: "market" },
        time_in_force: "gtc",
        requires_human_approval: false,
        futures: {
          reduce_only: reduceOnly,
          position_side: positionSide || undefined,
        },
      });
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      style={{
        marginTop: "0.55rem",
        paddingTop: "0.45rem",
        borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
      }}
    >
      <p className="tv-drawer__section-head" style={{ marginBottom: "0.25rem" }}>
        {t("app.tradingRangeFutures.sectionTitle")}
      </p>
      <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
        {t("app.tradingRangeFutures.intro")}
      </p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "0.45rem", alignItems: "flex-end", marginBottom: "0.35rem" }}>
        <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
          <span style={{ fontSize: "0.7rem" }}>{t("app.tradingRangeFutures.qtyLabel")}</span>
          <input
            type="text"
            className="mono tv-topstrip__input"
            style={{ maxWidth: "6rem" }}
            value={qty}
            onChange={(e) => setQty(e.target.value)}
            disabled={busy}
            autoComplete="off"
          />
        </label>
        <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
          <span style={{ fontSize: "0.7rem" }}>{t("app.tradingRangeFutures.positionSideLabel")}</span>
          <select
            className="tv-topstrip__select"
            value={positionSide}
            onChange={(e) => setPositionSide(e.target.value as "" | "LONG" | "SHORT")}
            disabled={busy}
          >
            <option value="">{t("app.tradingRangeFutures.positionSideOneWay")}</option>
            <option value="LONG">LONG</option>
            <option value="SHORT">SHORT</option>
          </select>
        </label>
      </div>
      <p className="muted mono" style={{ fontSize: "0.65rem", marginBottom: "0.35rem" }}>
        {sym} · futures
      </p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "0.35rem" }}>
        <button type="button" className="theme-toggle" disabled={busy} onClick={() => void runMarket("buy", false)}>
          {busy ? t("app.tradingRangeFutures.busy") : t("app.tradingRangeFutures.openLong")}
        </button>
        <button type="button" className="theme-toggle" disabled={busy} onClick={() => void runMarket("sell", false)}>
          {busy ? t("app.tradingRangeFutures.busy") : t("app.tradingRangeFutures.openShort")}
        </button>
        <button type="button" className="theme-toggle" disabled={busy} onClick={() => void runMarket("sell", true)}>
          {busy ? t("app.tradingRangeFutures.busy") : t("app.tradingRangeFutures.closeLong")}
        </button>
        <button type="button" className="theme-toggle" disabled={busy} onClick={() => void runMarket("buy", true)}>
          {busy ? t("app.tradingRangeFutures.busy") : t("app.tradingRangeFutures.closeShort")}
        </button>
      </div>
      {err ? <p className="err" style={{ fontSize: "0.72rem", marginTop: "0.35rem" }}>{err}</p> : null}
      <p className="muted" style={{ fontSize: "0.62rem", marginTop: "0.35rem", lineHeight: 1.45 }}>
        {t("app.tradingRangeFutures.riskNote")}
      </p>
    </div>
  );
}
