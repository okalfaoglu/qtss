import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  fetchDashboardPnlRollups,
  fetchMyBinanceOrders,
  fetchPaperBalance,
  fetchPaperFills,
  postDashboardPnlRebuild,
  type ExchangeOrderRowApi,
  type PaperFillRow,
  type PnlRollupRowApi,
} from "../api/client";
import { binanceVenueFillMetrics } from "../lib/binanceOrderFillMetrics";
import {
  impliedInitialQuoteFromFills,
  num,
  paperEquitySeries,
  paperFillsSortedAsc,
  paperPeriodBarsFromFills,
  paperSignedCashflow,
  rfc3339Since,
  startOfDayUtc,
  type PnlTimeScope,
} from "../lib/pnlDashboardMath";

function nextPeriodStart(scope: PnlTimeScope, start: Date): Date {
  const d = new Date(start);
  switch (scope) {
    case "instant":
    case "daily":
      d.setUTCDate(d.getUTCDate() + 1);
      return d;
    case "weekly":
      d.setUTCDate(d.getUTCDate() + 7);
      return d;
    case "monthly":
      d.setUTCMonth(d.getUTCMonth() + 1);
      return d;
    case "yearly":
      d.setUTCFullYear(d.getUTCFullYear() + 1);
      return d;
    default:
      d.setUTCDate(d.getUTCDate() + 1);
      return d;
  }
}

function rollupBucketForScope(scope: PnlTimeScope): string {
  if (scope === "instant") return "daily";
  return scope;
}

function maxBarsForScope(scope: PnlTimeScope): number {
  switch (scope) {
    case "instant":
      return 14;
    case "daily":
      return 35;
    case "weekly":
      return 26;
    case "monthly":
      return 36;
    case "yearly":
      return 10;
    default:
      return 30;
  }
}

function aggregateLiveRollups(rows: PnlRollupRowApi[]): { key: string; periodStart: Date; pnl: number; fees: number; volume: number; trades: number }[] {
  const m = new Map<string, { periodStart: Date; pnl: number; fees: number; volume: number; trades: number }>();
  for (const r of rows) {
    const ps = new Date(r.period_start);
    const k = r.period_start;
    const cur = m.get(k) ?? { periodStart: ps, pnl: 0, fees: 0, volume: 0, trades: 0 };
    cur.pnl += num(r.realized_pnl);
    cur.fees += num(r.fees);
    cur.volume += num(r.volume);
    cur.trades += r.trade_count;
    m.set(k, cur);
  }
  return [...m.values()]
    .sort((a, b) => a.periodStart.getTime() - b.periodStart.getTime())
    .map((x) => ({
      key: x.periodStart.toISOString(),
      periodStart: x.periodStart,
      pnl: x.pnl,
      fees: x.fees,
      volume: x.volume,
      trades: x.trades,
    }));
}

function formatPeriodLabel(iso: string, scope: PnlTimeScope): string {
  const d = new Date(iso);
  if (scope === "yearly") return String(d.getUTCFullYear());
  if (scope === "monthly") return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
  if (scope === "weekly") return `W ${d.toISOString().slice(0, 10)}`;
  return d.toISOString().slice(0, 10);
}

export function PnlDashboardPanel(props: { accessToken: string; canAdmin: boolean }) {
  const { t } = useTranslation();
  const { accessToken, canAdmin } = props;
  const [ledger, setLedger] = useState<"paper" | "live">("paper");
  const [scope, setScope] = useState<PnlTimeScope>("daily");
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [paperFills, setPaperFills] = useState<PaperFillRow[]>([]);
  const [paperBalance, setPaperBalance] = useState<number | null>(null);
  const [liveRollups, setLiveRollups] = useState<PnlRollupRowApi[]>([]);
  const [liveOrders, setLiveOrders] = useState<ExchangeOrderRowApi[]>([]);
  const [rebuildNote, setRebuildNote] = useState<string | null>(null);
  const [selectedBarKey, setSelectedBarKey] = useState<string | null>(null);

  const sinceIso = useMemo(() => rfc3339Since(scope), [scope]);

  const load = useCallback(async () => {
    setLoading(true);
    setErr(null);
    setRebuildNote(null);
    try {
      if (ledger === "paper") {
        const [fills, bal] = await Promise.all([
          fetchPaperFills(accessToken, 1000, sinceIso),
          fetchPaperBalance(accessToken).catch(() => null),
        ]);
        setPaperFills(fills);
        setPaperBalance(bal != null ? num(bal.quote_balance) : null);
        setLiveRollups([]);
        setLiveOrders([]);
      } else {
        const bucket = rollupBucketForScope(scope);
        const [roll, orders] = await Promise.all([
          fetchDashboardPnlRollups(accessToken, "live", bucket),
          fetchMyBinanceOrders(accessToken, { limit: 1000, sinceRfc3339: sinceIso }),
        ]);
        setLiveRollups(roll);
        setLiveOrders(orders);
        setPaperFills([]);
        setPaperBalance(null);
      }
      setSelectedBarKey(null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [accessToken, ledger, scope, sinceIso]);

  useEffect(() => {
    void load();
  }, [load]);

  const paperAsc = useMemo(() => paperFillsSortedAsc(paperFills), [paperFills]);

  const paperInitial = useMemo(() => {
    if (paperAsc.length === 0) return paperBalance ?? 10_000;
    const implied = impliedInitialQuoteFromFills(paperAsc);
    if (paperBalance != null && Math.abs(paperBalance - implied) < 1e-6) return implied;
    return implied;
  }, [paperAsc, paperBalance]);

  const paperEquity = useMemo(() => paperEquitySeries(paperAsc, paperInitial), [paperAsc, paperInitial]);

  const paperBars = useMemo(() => {
    const scopeForBars: PnlTimeScope = scope === "instant" ? "daily" : scope;
    return paperPeriodBarsFromFills(paperAsc, scopeForBars, maxBarsForScope(scope));
  }, [paperAsc, scope]);

  const paperBarChartData = useMemo(
    () =>
      paperBars.map((b) => ({
        key: b.key,
        label: formatPeriodLabel(b.key, scope === "instant" ? "daily" : scope),
        pnl: b.pnl,
        fees: b.fees,
        volume: b.volume,
        trades: b.trades,
      })),
    [paperBars, scope],
  );

  const paperLineData = useMemo(() => {
    if (scope === "instant") {
      const cutoff = Date.now() - 24 * 3600 * 1000;
      return paperEquity.points
        .filter((p) => new Date(p.t).getTime() >= cutoff)
        .map((p) => ({ t: p.t, equity: p.equity }));
    }
    return paperEquity.points.map((p) => ({ t: p.t, equity: p.equity }));
  }, [paperEquity.points, scope]);

  const liveAgg = useMemo(() => aggregateLiveRollups(liveRollups), [liveRollups]);
  const liveBarChartData = useMemo(() => {
    const max = maxBarsForScope(scope);
    const slice = liveAgg.length > max ? liveAgg.slice(-max) : liveAgg;
    return slice.map((b) => ({
      key: b.key,
      label: formatPeriodLabel(b.key, scope === "instant" ? "daily" : scope),
      pnl: b.pnl,
      fees: b.fees,
      volume: b.volume,
      trades: b.trades,
    }));
  }, [liveAgg, scope]);

  const liveLineData = useMemo(() => {
    let acc = 0;
    return liveBarChartData.map((r) => {
      acc += r.pnl;
      return { t: r.label, cumulativePnl: acc };
    });
  }, [liveBarChartData]);

  const selectedPeriodPaperFills = useMemo(() => {
    if (!selectedBarKey || ledger !== "paper") return [];
    const start = new Date(selectedBarKey);
    const end = nextPeriodStart(scope === "instant" ? "daily" : scope, start);
    return paperAsc.filter((f) => {
      const ts = new Date(f.created_at).getTime();
      return ts >= start.getTime() && ts < end.getTime();
    });
  }, [ledger, paperAsc, selectedBarKey, scope]);

  const selectedPeriodLiveOrders = useMemo(() => {
    if (!selectedBarKey || ledger !== "live") return [];
    const start = new Date(selectedBarKey);
    const end = nextPeriodStart(scope === "instant" ? "daily" : scope, start);
    return liveOrders.filter((o) => {
      const ts = new Date(o.updated_at).getTime();
      return ts >= start.getTime() && ts < end.getTime();
    });
  }, [ledger, liveOrders, selectedBarKey, scope]);

  const instantPaperKpi = useMemo(() => {
    const cutoff = Date.now() - 24 * 3600 * 1000;
    let cf = 0;
    let n = 0;
    for (const f of paperAsc) {
      if (new Date(f.created_at).getTime() < cutoff) continue;
      cf += paperSignedCashflow(f);
      n += 1;
    }
    return { cashflow24h: cf, trades24h: n };
  }, [paperAsc]);

  const onRebuild = async () => {
    if (!canAdmin) return;
    setRebuildNote(null);
    try {
      const s = await postDashboardPnlRebuild(accessToken);
      setRebuildNote(
        t("app.pnlDashboard.rebuildOk", {
          scanned: s.orders_scanned,
          fills: s.orders_with_fills,
          rows: s.rollup_rows_written,
        }),
      );
      await load();
    } catch (e) {
      setRebuildNote(e instanceof Error ? e.message : String(e));
    }
  };

  const barData = ledger === "paper" ? paperBarChartData : liveBarChartData;

  return (
    <div className="card" style={{ marginTop: "0.5rem" }}>
      <div className="tv-drawer__section-head" style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem", alignItems: "center" }}>
        <span>{t("app.pnlDashboard.title")}</span>
        <button type="button" className="theme-toggle" onClick={() => void load()} disabled={loading}>
          {loading ? "…" : t("app.pnlDashboard.refresh")}
        </button>
      </div>

      <div style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem", marginTop: "0.75rem" }}>
        <label className="muted">
          {t("app.pnlDashboard.ledger")}
          <select
            className="tv-topstrip__input"
            style={{ marginLeft: "0.35rem" }}
            value={ledger}
            onChange={(e) => setLedger(e.target.value === "live" ? "live" : "paper")}
          >
            <option value="paper">{t("app.pnlDashboard.paper")}</option>
            <option value="live">{t("app.pnlDashboard.live")}</option>
          </select>
        </label>
        {SCOPE_TABS.map((s) => (
          <button
            key={s}
            type="button"
            className={`tv-settings__tab ${scope === s ? "is-active" : ""}`}
            onClick={() => setScope(s)}
          >
            {t(`app.pnlDashboard.scope.${s}`)}
          </button>
        ))}
      </div>

      {err ? <p className="tv-pnl-neg" style={{ marginTop: "0.5rem" }}>{err}</p> : null}
      {rebuildNote ? <p className="muted" style={{ marginTop: "0.5rem" }}>{rebuildNote}</p> : null}

      {ledger === "paper" && scope === "instant" ? (
        <div style={{ marginTop: "0.75rem", display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(140px, 1fr))", gap: "0.5rem" }}>
          <div className="mono">
            <div className="muted">{t("app.pnlDashboard.kpi.quoteBalance")}</div>
            <div>{paperBalance != null ? paperBalance.toFixed(2) : "—"}</div>
          </div>
          <div className="mono">
            <div className="muted">{t("app.pnlDashboard.kpi.cashflow24h")}</div>
            <div className={instantPaperKpi.cashflow24h >= 0 ? "tv-pnl-pos" : "tv-pnl-neg"}>
              {instantPaperKpi.cashflow24h.toFixed(2)}
            </div>
          </div>
          <div className="mono">
            <div className="muted">{t("app.pnlDashboard.kpi.trades24h")}</div>
            <div>{instantPaperKpi.trades24h}</div>
          </div>
        </div>
      ) : null}

      {ledger === "live" && canAdmin ? (
        <div style={{ marginTop: "0.5rem" }}>
          <button type="button" className="theme-toggle" onClick={() => void onRebuild()}>
            {t("app.pnlDashboard.rebuildRollups")}
          </button>
          <p className="muted" style={{ marginTop: "0.35rem", fontSize: "0.85rem" }}>
            {t("app.pnlDashboard.liveRollupHint")}
          </p>
        </div>
      ) : null}

      <p className="muted" style={{ marginTop: "0.5rem", fontSize: "0.85rem" }}>
        {ledger === "paper" ? t("app.pnlDashboard.paperHint") : t("app.pnlDashboard.liveHint")}
      </p>

      <div style={{ width: "100%", height: 260, marginTop: "0.75rem" }}>
        <ResponsiveContainer>
          <BarChart
            data={barData}
            margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
            onClick={(state) => {
              const p = state?.activePayload?.[0]?.payload as { key?: string } | undefined;
              if (p?.key) setSelectedBarKey((k) => (k === p.key ? null : p.key));
            }}
          >
            <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
            <XAxis dataKey="label" tick={{ fontSize: 10 }} />
            <YAxis tick={{ fontSize: 10 }} />
            <Tooltip />
            <Legend />
            <Bar dataKey="pnl" name={t("app.pnlDashboard.chart.pnl")} fill="var(--pnl-bar, #26a69a)" />
            <Bar dataKey="fees" name={t("app.pnlDashboard.chart.fees")} fill="var(--fee-bar, #ef5350)" />
          </BarChart>
        </ResponsiveContainer>
      </div>

      <div style={{ width: "100%", height: 220, marginTop: "1rem" }}>
        <ResponsiveContainer>
          <LineChart data={ledger === "paper" ? paperLineData : liveLineData} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
            <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
            <XAxis dataKey="t" tick={{ fontSize: 9 }} tickFormatter={(v) => String(v).slice(0, 10)} />
            <YAxis tick={{ fontSize: 10 }} />
            <Tooltip />
            <Legend />
            {ledger === "paper" ? (
              <Line
                type="monotone"
                dataKey="equity"
                name={t("app.pnlDashboard.chart.equityPaper")}
                stroke="var(--equity-line, #42a5f5)"
                dot={false}
                strokeWidth={2}
              />
            ) : (
              <Line
                type="monotone"
                dataKey="cumulativePnl"
                name={t("app.pnlDashboard.chart.cumulativePnlRollup")}
                stroke="var(--equity-line, #42a5f5)"
                dot={false}
                strokeWidth={2}
              />
            )}
          </LineChart>
        </ResponsiveContainer>
      </div>

      {selectedBarKey ? (
        <div style={{ marginTop: "1rem" }}>
          <p className="tv-drawer__section-head">
            {t("app.pnlDashboard.drilldownTitle")}{" "}
            <button type="button" className="tv-icon-btn" onClick={() => setSelectedBarKey(null)} aria-label={t("app.pnlDashboard.clearSelection")}>
              ×
            </button>
          </p>
          {ledger === "paper" ? (
            <div style={{ overflowX: "auto" }}>
              <table style={{ width: "100%", fontSize: "0.85rem", borderCollapse: "collapse" }}>
                <thead>
                  <tr>
                    <th>{t("app.pnlDashboard.col.time")}</th>
                    <th>{t("app.pnlDashboard.col.symbol")}</th>
                    <th>{t("app.pnlDashboard.col.side")}</th>
                    <th>{t("app.pnlDashboard.col.qty")}</th>
                    <th>{t("app.pnlDashboard.col.price")}</th>
                    <th>{t("app.pnlDashboard.col.fee")}</th>
                    <th>{t("app.pnlDashboard.col.cashflow")}</th>
                    <th>{t("app.pnlDashboard.col.quoteAfter")}</th>
                  </tr>
                </thead>
                <tbody>
                  {selectedPeriodPaperFills.map((f) => (
                    <tr key={f.id}>
                      <td className="mono">{f.created_at.slice(0, 19)}Z</td>
                      <td>{f.symbol}</td>
                      <td>{f.side}</td>
                      <td className="mono">{String(f.quantity)}</td>
                      <td className="mono">{String(f.avg_price)}</td>
                      <td className="mono">{String(f.fee)}</td>
                      <td className={paperSignedCashflow(f) >= 0 ? "tv-pnl-pos" : "tv-pnl-neg"}>
                        {paperSignedCashflow(f).toFixed(4)}
                      </td>
                      <td className="mono">{String(f.quote_balance_after)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {selectedPeriodPaperFills.length === 0 ? <p className="muted">{t("app.pnlDashboard.noTradesInPeriod")}</p> : null}
            </div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table style={{ width: "100%", fontSize: "0.85rem", borderCollapse: "collapse" }}>
                <thead>
                  <tr>
                    <th>{t("app.pnlDashboard.col.time")}</th>
                    <th>{t("app.pnlDashboard.col.symbol")}</th>
                    <th>{t("app.pnlDashboard.col.segment")}</th>
                    <th>{t("app.pnlDashboard.col.status")}</th>
                    <th>{t("app.pnlDashboard.col.quoteQty")}</th>
                    <th>{t("app.pnlDashboard.col.fee")}</th>
                  </tr>
                </thead>
                <tbody>
                  {selectedPeriodLiveOrders.map((o) => {
                    const m = binanceVenueFillMetrics(o.venue_response);
                    return (
                      <tr key={o.id}>
                        <td className="mono">{o.updated_at.slice(0, 19)}Z</td>
                        <td>{o.symbol}</td>
                        <td>{o.segment}</td>
                        <td>{m?.status ?? o.status}</td>
                        <td className="mono">{m ? m.quoteQty.toFixed(4) : "—"}</td>
                        <td className="mono">{m ? m.fee.toFixed(6) : "—"}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {selectedPeriodLiveOrders.length === 0 ? <p className="muted">{t("app.pnlDashboard.noTradesInPeriod")}</p> : null}
            </div>
          )}
        </div>
      ) : (
        <p className="muted" style={{ marginTop: "0.75rem" }}>
          {t("app.pnlDashboard.clickBarHint")}
        </p>
      )}
    </div>
  );
}

const SCOPE_TABS: PnlTimeScope[] = ["instant", "daily", "weekly", "monthly", "yearly"];
