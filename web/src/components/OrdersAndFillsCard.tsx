import { useEffect, useMemo, useState } from "react";
import {
  fetchMyBinanceOrders,
  fetchMyBinanceFills,
  fetchPaperFills,
  postMyBinancePlaceOrder,
  postReconcileBinanceFutures,
  postReconcileBinanceSpot,
  type ExchangeOrderRowApi,
  type ExchangeFillRowApi,
  type PaperFillRow,
  type ReconcileReportApi,
} from "../api/client";

type Props = {
  accessToken: string | null;
};

function asIso(s: string): string {
  const d = new Date(s);
  return Number.isFinite(d.getTime()) ? d.toISOString() : s;
}

function isoFromLocalInput(s: string): string {
  const d = new Date(s);
  return Number.isFinite(d.getTime()) ? d.toISOString() : "";
}

function statusBadge(statusRaw: string | null | undefined): { text: string; bg: string; fg: string; border: string } {
  const t = (statusRaw ?? "").trim().toLowerCase();
  if (!t) {
    return { text: "-", bg: "transparent", fg: "var(--muted)", border: "var(--border)" };
  }
  if (t.includes("filled") || t.includes("closed") || t.includes("done")) {
    return { text: t, bg: "rgba(21,128,61,0.12)", fg: "#15803d", border: "rgba(21,128,61,0.35)" };
  }
  if (t.includes("cancel") || t.includes("reject") || t.includes("fail") || t.includes("expired")) {
    return { text: t, bg: "rgba(185,28,28,0.12)", fg: "#b91c1c", border: "rgba(185,28,28,0.35)" };
  }
  if (t.includes("new") || t.includes("open") || t.includes("accept") || t.includes("pending")) {
    return { text: t, bg: "rgba(59,130,246,0.12)", fg: "#2563eb", border: "rgba(59,130,246,0.35)" };
  }
  return { text: t, bg: "rgba(148,163,184,0.12)", fg: "var(--fg)", border: "rgba(148,163,184,0.35)" };
}

function statusGroupOf(statusRaw: string | null | undefined): "open" | "filled" | "canceled" | "other" {
  const t = (statusRaw ?? "").trim().toLowerCase();
  if (!t) return "other";
  if (t.includes("filled") || t.includes("closed") || t.includes("done")) return "filled";
  if (t.includes("cancel") || t.includes("reject") || t.includes("fail") || t.includes("expired")) return "canceled";
  if (t.includes("new") || t.includes("open") || t.includes("accept") || t.includes("pending")) return "open";
  return "other";
}

export function OrdersAndFillsCard({ accessToken }: Props) {
  const [tab, setTab] = useState<"exchange_orders" | "exchange_fills" | "paper_fills">("exchange_orders");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [sort, setSort] = useState<"newest" | "oldest">("newest");
  const [limit, setLimit] = useState(200);
  const [query, setQuery] = useState("");
  const [sinceLocal, setSinceLocal] = useState("");
  const [exchangeFilter, setExchangeFilter] = useState("");
  const [segmentFilter, setSegmentFilter] = useState("");
  const [statusGroup, setStatusGroup] = useState<"all" | "open" | "filled" | "canceled">("all");
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [autoRefreshSec, setAutoRefreshSec] = useState(10);
  const [orders, setOrders] = useState<ExchangeOrderRowApi[]>([]);
  const [exchangeFills, setExchangeFills] = useState<ExchangeFillRowApi[]>([]);
  const [fills, setFills] = useState<PaperFillRow[]>([]);
  const [reconcileBusy, setReconcileBusy] = useState(false);
  const [reconcileReport, setReconcileReport] = useState<ReconcileReportApi | null>(null);

  // Minimal “Place order” (Binance futures trailing stop market)
  const [placeBusy, setPlaceBusy] = useState(false);
  const [placeSide, setPlaceSide] = useState<"buy" | "sell">("sell");
  const [placeSymbol, setPlaceSymbol] = useState("BTCUSDT");
  const [placeQty, setPlaceQty] = useState("0.001");
  const [placeCallbackRate, setPlaceCallbackRate] = useState("1");
  const [placeReduceOnly, setPlaceReduceOnly] = useState(true);
  const [placePositionSide, setPlacePositionSide] = useState<"" | "LONG" | "SHORT">("");

  const refresh = async () => {
    setErr("");
    if (!accessToken) {
      setErr("Giriş gerekli (JWT).");
      return;
    }
    setBusy(true);
    try {
      const sinceIso = sinceLocal.trim() ? isoFromLocalInput(sinceLocal.trim()) : "";
      if (tab === "exchange_orders") {
        const rows = await fetchMyBinanceOrders(accessToken, { limit, sinceRfc3339: sinceIso || null });
        setOrders(rows);
      } else if (tab === "exchange_fills") {
        const rows = await fetchMyBinanceFills(accessToken, limit);
        setExchangeFills(rows);
      } else {
        const rows = await fetchPaperFills(accessToken, limit, sinceIso || null);
        setFills(rows);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onPlaceTrailingStopMarketFutures = async () => {
    setErr("");
    if (!accessToken) {
      setErr("Giriş gerekli (JWT).");
      return;
    }
    const symbol = placeSymbol.trim().toUpperCase();
    const qty = Number(placeQty);
    const cb = Number(placeCallbackRate);
    if (!symbol) return setErr("symbol gerekli");
    if (!Number.isFinite(qty) || qty <= 0) return setErr("quantity > 0 olmalı");
    if (!Number.isFinite(cb) || cb <= 0) return setErr("callback_rate > 0 olmalı");
    setPlaceBusy(true);
    try {
      const intent = {
        instrument: { exchange: "binance", segment: "futures", symbol },
        side: placeSide,
        quantity: String(qty),
        order_type: { type: "trailing_stop_market", callback_rate: String(cb) },
        time_in_force: "gtc",
        requires_human_approval: false,
        futures: {
          position_side: placePositionSide || undefined,
          reduce_only: !!placeReduceOnly,
        },
      };
      await postMyBinancePlaceOrder(accessToken, intent);
      await refresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setPlaceBusy(false);
    }
  };

  const onReconcile = async (segment: "spot" | "futures") => {
    setErr("");
    if (!accessToken) {
      setErr("Giriş gerekli (JWT).");
      return;
    }
    setReconcileBusy(true);
    try {
      const rep = segment === "futures"
        ? await postReconcileBinanceFutures(accessToken)
        : await postReconcileBinanceSpot(accessToken);
      setReconcileReport(rep);
      await refresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setReconcileBusy(false);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  useEffect(() => {
    if (!autoRefresh) return;
    if (!accessToken) return;
    const ms = Math.min(120_000, Math.max(3_000, Math.floor(autoRefreshSec * 1000)));
    const id = window.setInterval(() => {
      if (!busy) void refresh();
    }, ms);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoRefresh, autoRefreshSec, accessToken, tab, busy]);

  const filteredOrders = useMemo(() => {
    const q = query.trim().toLowerCase();
    const ex = exchangeFilter.trim().toLowerCase();
    const seg = segmentFilter.trim().toLowerCase();
    let rows = orders.slice();
    if (ex) rows = rows.filter((r) => (r.exchange ?? "").toLowerCase().includes(ex));
    if (seg) rows = rows.filter((r) => (r.segment ?? "").toLowerCase().includes(seg));
    if (statusGroup !== "all") {
      rows = rows.filter((r) => statusGroupOf(r.status) === statusGroup);
    }
    if (q) {
      rows = rows.filter((r) => {
        const sym = (r.symbol ?? "").toLowerCase();
        const st = (r.status ?? "").toLowerCase();
        const cid = (r.client_order_id ?? "").toLowerCase();
        return sym.includes(q) || st.includes(q) || cid.includes(q);
      });
    }
    rows.sort((a, b) => {
      const ta = new Date(a.created_at).getTime();
      const tb = new Date(b.created_at).getTime();
      return sort === "newest" ? tb - ta : ta - tb;
    });
    return rows;
  }, [orders, query, sort]);

  const filteredFills = useMemo(() => {
    const q = query.trim().toLowerCase();
    const ex = exchangeFilter.trim().toLowerCase();
    const seg = segmentFilter.trim().toLowerCase();
    let rows = fills.slice();
    if (ex) rows = rows.filter((r) => (r.exchange ?? "").toLowerCase().includes(ex));
    if (seg) rows = rows.filter((r) => (r.segment ?? "").toLowerCase().includes(seg));
    if (q) {
      rows = rows.filter((r) => {
        const sym = (r.symbol ?? "").toLowerCase();
        const side = (r.side ?? "").toLowerCase();
        const cid = (r.client_order_id ?? "").toLowerCase();
        return sym.includes(q) || side.includes(q) || cid.includes(q);
      });
    }
    rows.sort((a, b) => {
      const ta = new Date(a.created_at).getTime();
      const tb = new Date(b.created_at).getTime();
      return sort === "newest" ? tb - ta : ta - tb;
    });
    return rows;
  }, [fills, query, sort]);

  const filteredExchangeFills = useMemo(() => {
    const q = query.trim().toLowerCase();
    const ex = exchangeFilter.trim().toLowerCase();
    const seg = segmentFilter.trim().toLowerCase();
    let rows = exchangeFills.slice();
    if (ex) rows = rows.filter((r) => (r.exchange ?? "").toLowerCase().includes(ex));
    if (seg) rows = rows.filter((r) => (r.segment ?? "").toLowerCase().includes(seg));
    if (q) {
      rows = rows.filter((r) => {
        const sym = (r.symbol ?? "").toLowerCase();
        const feeA = (r.fee_asset ?? "").toLowerCase();
        return sym.includes(q) || feeA.includes(q) || String(r.venue_order_id).includes(q);
      });
    }
    rows.sort((a, b) => {
      const ta = new Date(a.event_time).getTime();
      const tb = new Date(b.event_time).getTime();
      return sort === "newest" ? tb - ta : ta - tb;
    });
    return rows;
  }, [exchangeFills, exchangeFilter, segmentFilter, query, sort]);

  return (
    <div className="card">
      <p className="tv-drawer__section-head">Emirler & Fills</p>
      <div className="card" style={{ marginTop: "0.75rem" }}>
        <p className="tv-drawer__section-head">Emir gönder (Binance futures)</p>
        <p className="muted" style={{ marginTop: 0, fontSize: "0.82rem" }}>
          Minimal: <code className="mono">TrailingStopMarket</code> —{" "}
          <code className="mono">POST /api/v1/orders/binance/place</code>
        </p>
        <div style={{ display: "flex", gap: "0.45rem", flexWrap: "wrap", alignItems: "end" }}>
          <label className="muted">
            <span>side</span>
            <select className="tv-topstrip__select" value={placeSide} onChange={(e) => setPlaceSide(e.target.value as any)}>
              <option value="buy">buy</option>
              <option value="sell">sell</option>
            </select>
          </label>
          <label className="muted">
            <span>symbol</span>
            <input className="tv-topstrip__input" value={placeSymbol} onChange={(e) => setPlaceSymbol(e.target.value)} style={{ width: "9rem" }} />
          </label>
          <label className="muted">
            <span>qty</span>
            <input className="tv-topstrip__input" value={placeQty} onChange={(e) => setPlaceQty(e.target.value)} style={{ width: "7rem" }} />
          </label>
          <label className="muted">
            <span>callback_rate</span>
            <input
              className="tv-topstrip__input"
              value={placeCallbackRate}
              onChange={(e) => setPlaceCallbackRate(e.target.value)}
              style={{ width: "7rem" }}
            />
          </label>
          <label className="muted">
            <span>position_side</span>
            <select
              className="tv-topstrip__select"
              value={placePositionSide}
              onChange={(e) => setPlacePositionSide(e.target.value as any)}
            >
              <option value="">(BOTH)</option>
              <option value="LONG">LONG</option>
              <option value="SHORT">SHORT</option>
            </select>
          </label>
          <label className="muted" style={{ display: "flex", gap: "0.35rem", alignItems: "center" }}>
            <input type="checkbox" checked={placeReduceOnly} onChange={(e) => setPlaceReduceOnly(e.target.checked)} />
            <span>reduce_only</span>
          </label>
          <button
            type="button"
            className="theme-toggle"
            onClick={() => void onPlaceTrailingStopMarketFutures()}
            disabled={placeBusy || busy}
          >
            {placeBusy ? "Gönderiliyor…" : "Place trailing stop (futures)"}
          </button>
        </div>
      </div>

      <div style={{ display: "flex", gap: "0.35rem", flexWrap: "wrap", alignItems: "center" }}>
        <button
          type="button"
          className={`tv-settings__tab ${tab === "exchange_orders" ? "is-active" : ""}`}
          onClick={() => setTab("exchange_orders")}
        >
          exchange_orders
        </button>
        <button
          type="button"
          className={`tv-settings__tab ${tab === "exchange_fills" ? "is-active" : ""}`}
          onClick={() => setTab("exchange_fills")}
        >
          exchange_fills
        </button>
        <button
          type="button"
          className={`tv-settings__tab ${tab === "paper_fills" ? "is-active" : ""}`}
          onClick={() => setTab("paper_fills")}
        >
          paper_fills
        </button>
      </div>

      <div className="tv-settings__fields" style={{ marginTop: "0.55rem" }}>
        <label className="muted">
          <span>Filter</span>
          <input className="tv-topstrip__input mono" value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <label className="muted">
          <span>Exchange</span>
          <input className="tv-topstrip__input mono" value={exchangeFilter} onChange={(e) => setExchangeFilter(e.target.value)} />
        </label>
        <label className="muted">
          <span>Segment</span>
          <input className="tv-topstrip__input mono" value={segmentFilter} onChange={(e) => setSegmentFilter(e.target.value)} />
        </label>
        <label className="muted">
          <span>Since</span>
          <input className="tv-topstrip__input mono" type="datetime-local" value={sinceLocal} onChange={(e) => setSinceLocal(e.target.value)} />
        </label>
        {tab === "exchange_orders" ? (
          <label className="muted">
            <span>Status</span>
            <select className="tv-topstrip__select" value={statusGroup} onChange={(e) => setStatusGroup(e.target.value as any)}>
              <option value="all">All</option>
              <option value="open">Open</option>
              <option value="filled">Filled</option>
              <option value="canceled">Canceled</option>
            </select>
          </label>
        ) : null}
        <label className="muted">
          <span>Sort</span>
          <select className="tv-topstrip__select" value={sort} onChange={(e) => setSort(e.target.value as any)}>
            <option value="newest">Newest</option>
            <option value="oldest">Oldest</option>
          </select>
        </label>
        <label className="muted">
          <span>Limit</span>
          <input
            className="tv-topstrip__input mono"
            type="number"
            min={1}
            max={1000}
            value={limit}
            onChange={(e) => setLimit(Math.min(1000, Math.max(1, parseInt(e.target.value, 10) || 200)))}
          />
        </label>
        <label className="muted" style={{ display: "flex", alignItems: "center", gap: "0.45rem", marginTop: "1.25rem" }}>
          <input type="checkbox" checked={autoRefresh} onChange={(e) => setAutoRefresh(e.target.checked)} />
          <span>Auto-refresh</span>
        </label>
        <label className="muted">
          <span>Every (sec)</span>
          <input
            className="tv-topstrip__input mono"
            type="number"
            min={3}
            max={120}
            value={autoRefreshSec}
            onChange={(e) => setAutoRefreshSec(Math.min(120, Math.max(3, parseInt(e.target.value, 10) || 10)))}
            disabled={!autoRefresh}
          />
        </label>
      </div>

      <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.55rem", alignItems: "center" }}>
        <button type="button" className="theme-toggle" onClick={() => void refresh()} disabled={busy}>
          {busy ? "Yükleniyor…" : "Refresh"}
        </button>
        {tab === "exchange_orders" ? (
          <>
            <button type="button" className="theme-toggle" onClick={() => void onReconcile("spot")} disabled={reconcileBusy}>
              {reconcileBusy ? "Reconcile…" : "Reconcile spot"}
            </button>
            <button type="button" className="theme-toggle" onClick={() => void onReconcile("futures")} disabled={reconcileBusy}>
              {reconcileBusy ? "Reconcile…" : "Reconcile futures"}
            </button>
          </>
        ) : null}
        <span className="muted" style={{ fontSize: "0.8rem" }}>
          {tab === "exchange_orders" ? (
            <code>GET /api/v1/orders/binance</code>
          ) : tab === "exchange_fills" ? (
            <code>GET /api/v1/orders/binance/fills</code>
          ) : (
            <code>GET /api/v1/orders/dry/fills</code>
          )}
        </span>
      </div>

      {err ? <p className="err" style={{ marginTop: "0.45rem" }}>{err}</p> : null}
      {reconcileReport ? (
        <p className="muted mono" style={{ marginTop: "0.35rem", fontSize: "0.72rem" }}>
          reconcile: venue={reconcileReport.venue} · mismatches={reconcileReport.mismatches} · updates={reconcileReport.status_updates_applied ?? 0}
        </p>
      ) : null}

      {tab === "exchange_orders" ? (
        <div style={{ marginTop: "0.75rem", overflowX: "auto" }}>
          <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>created</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>exchange</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>segment</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>symbol</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>status</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>client_order_id</th>
              </tr>
            </thead>
            <tbody>
              {filteredOrders.map((r) => (
                <tr key={r.id}>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{asIso(r.created_at)}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.exchange}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.segment}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.symbol}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>
                    {(() => {
                      const b = statusBadge(r.status);
                      return (
                        <span
                          style={{
                            display: "inline-block",
                            padding: "0.05rem 0.35rem",
                            borderRadius: "999px",
                            border: `1px solid ${b.border}`,
                            background: b.bg,
                            color: b.fg,
                            fontSize: "0.7rem",
                          }}
                        >
                          {b.text}
                        </span>
                      );
                    })()}
                  </td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.client_order_id}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : tab === "exchange_fills" ? (
        <div style={{ marginTop: "0.75rem", overflowX: "auto" }}>
          <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>event</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>exchange</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>segment</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>symbol</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>qty</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>price</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>fee</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>fee_asset</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>venue_order_id</th>
              </tr>
            </thead>
            <tbody>
              {filteredExchangeFills.map((r) => (
                <tr key={r.id}>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{asIso(r.event_time)}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.exchange}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.segment}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.symbol}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.fill_quantity ?? "-"}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.fill_price ?? "-"}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.fee ?? "-"}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.fee_asset ?? "-"}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.venue_order_id}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div style={{ marginTop: "0.75rem", overflowX: "auto" }}>
          <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>created</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>exchange</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>segment</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>symbol</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>side</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>qty</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>avg_price</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>fee</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>client_order_id</th>
              </tr>
            </thead>
            <tbody>
              {filteredFills.map((r) => (
                <tr key={r.id}>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{asIso(r.created_at)}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.exchange}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.segment}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.symbol}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.side}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.quantity}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.avg_price}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{r.fee}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{r.client_order_id}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

