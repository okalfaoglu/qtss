import { useEffect, useMemo, useRef, useState } from "react";
import { ColorType, createChart } from "lightweight-charts";
import type { ISeriesApi, IChartApi, UTCTimestamp } from "lightweight-charts";
import { useTranslation } from "react-i18next";
import {
  fetchDashboardPnlEquityCurve,
  fetchDashboardPnlRollups,
  fetchMyBinanceOrders,
  type PnlEquityPointApi,
  type PnlRollupRowApi,
} from "../api/client";

type PnlBucket = "instant" | "daily" | "weekly" | "monthly" | "yearly";
type PnlLedger = "live" | "dry";

type MetricPoint = { time: UTCTimestamp; value: number };
type EquityPoint = { time: UTCTimestamp; value: number };
type HistPoint = { time: UTCTimestamp; value: number; color?: string };

function parseApiNumber(v: string | number): number {
  if (typeof v === "number") return Number.isFinite(v) ? v : 0;
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
}

function strictAsc(points: MetricPoint[]): MetricPoint[] {
  const sorted = [...points].sort((a, b) => (a.time as number) - (b.time as number));
  const out: MetricPoint[] = [];
  for (const p of sorted) {
    const t = p.time as number;
    if (!Number.isFinite(t)) continue;
    const last = out[out.length - 1];
    if (!last) out.push(p);
    else if ((last.time as number) < t) out.push(p);
    else if ((last.time as number) === t) out[out.length - 1] = p;
  }
  return out;
}

function floorToMinuteUtc(tsMs: number): number {
  return Math.floor(tsMs / 60_000) * 60_000;
}

function toUtcTimestampSeconds(tsMs: number): UTCTimestamp {
  return Math.floor(tsMs / 1000) as UTCTimestamp;
}

function rollupsToSeries(rows: PnlRollupRowApi[]): {
  tradeCount: MetricPoint[];
  closedTrades: MetricPoint[];
  netPnl: MetricPoint[];
} {
  const agg = new Map<number, { tradeCount: number; closedTrades: number; netPnl: number }>();
  for (const r of rows) {
    const t = Date.parse(r.period_start);
    if (!Number.isFinite(t)) continue;
    const cur = agg.get(t) ?? { tradeCount: 0, closedTrades: 0, netPnl: 0 };
    cur.tradeCount += Number.isFinite(r.trade_count) ? r.trade_count : 0;
    cur.closedTrades += Number.isFinite(r.closed_trade_count) ? r.closed_trade_count : 0;
    const realized = parseApiNumber(r.realized_pnl);
    const fees = parseApiNumber(r.fees);
    cur.netPnl += realized - fees;
    agg.set(t, cur);
  }
  const tradeCount: MetricPoint[] = [];
  const closedTrades: MetricPoint[] = [];
  const netPnl: MetricPoint[] = [];
  for (const [tMs, v] of agg) {
    const time = toUtcTimestampSeconds(tMs);
    tradeCount.push({ time, value: v.tradeCount });
    closedTrades.push({ time, value: v.closedTrades });
    netPnl.push({ time, value: v.netPnl });
  }
  return {
    tradeCount: strictAsc(tradeCount),
    closedTrades: strictAsc(closedTrades),
    netPnl: strictAsc(netPnl),
  };
}

function equityToSeries(points: PnlEquityPointApi[]): { equity: EquityPoint[]; periodNet: HistPoint[] } {
  const equity: EquityPoint[] = [];
  const periodNet: HistPoint[] = [];
  for (const p of points) {
    const tMs = Date.parse(p.t);
    if (!Number.isFinite(tMs)) continue;
    const time = toUtcTimestampSeconds(tMs);
    const e = parseApiNumber(p.equity);
    const net = parseApiNumber(p.realized_pnl) - parseApiNumber(p.fees);
    equity.push({ time, value: e });
    periodNet.push({ time, value: net, color: net >= 0 ? "rgba(52, 211, 153, 0.65)" : "rgba(251, 113, 133, 0.65)" });
  }
  return { equity: strictAsc(equity), periodNet: strictAsc(periodNet) };
}

function useMiniChart(opts: { points: MetricPoint[]; accent: string; isMoney?: boolean }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);

  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;

    const chart = createChart(el, {
      width: el.clientWidth,
      height: 180,
      layout: {
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: "rgba(180, 170, 155, 0.9)",
      },
      grid: {
        horzLines: { color: "rgba(120, 110, 95, 0.16)" },
        vertLines: { color: "rgba(120, 110, 95, 0.08)" },
      },
      rightPriceScale: { borderVisible: false },
      timeScale: { borderVisible: false },
      crosshair: { vertLine: { visible: false }, horzLine: { visible: false } },
    });
    chartRef.current = chart;

    const series = chart.addAreaSeries({
      lineColor: opts.accent,
      topColor: `${opts.accent}33`,
      bottomColor: `${opts.accent}00`,
      lineWidth: 2,
    });
    seriesRef.current = series;
    series.setData(opts.points);
    chart.timeScale().fitContent();

    const ro = new ResizeObserver(() => {
      const w = el.clientWidth;
      if (w > 10) chart.applyOptions({ width: w });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, [opts.accent]);

  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!chart || !series) return;
    series.setData(opts.points);
    chart.timeScale().fitContent();
  }, [opts.points]);

  return hostRef;
}

function useEquityChart(opts: { equity: EquityPoint[]; pnlBars: HistPoint[] }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const equityRef = useRef<ISeriesApi<"Line"> | null>(null);
  const histRef = useRef<ISeriesApi<"Histogram"> | null>(null);

  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const chart = createChart(el, {
      width: el.clientWidth,
      height: 220,
      layout: { background: { type: ColorType.Solid, color: "transparent" }, textColor: "rgba(180, 170, 155, 0.9)" },
      grid: { horzLines: { color: "rgba(120, 110, 95, 0.16)" }, vertLines: { color: "rgba(120, 110, 95, 0.08)" } },
      rightPriceScale: { borderVisible: false },
      timeScale: { borderVisible: false },
      crosshair: { vertLine: { visible: false }, horzLine: { visible: false } },
    });
    chartRef.current = chart;

    const hist = chart.addHistogramSeries({
      priceScaleId: "",
      base: 0,
      color: "rgba(251, 113, 133, 0.55)",
    });
    histRef.current = hist;

    const eq = chart.addLineSeries({ color: "#fbbf24", lineWidth: 2 });
    equityRef.current = eq;

    hist.setData(opts.pnlBars);
    eq.setData(opts.equity);
    chart.timeScale().fitContent();

    const ro = new ResizeObserver(() => {
      const w = el.clientWidth;
      if (w > 10) chart.applyOptions({ width: w });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      equityRef.current = null;
      histRef.current = null;
    };
  }, []);

  useEffect(() => {
    const chart = chartRef.current;
    const eq = equityRef.current;
    const hist = histRef.current;
    if (!chart || !eq || !hist) return;
    hist.setData(opts.pnlBars);
    eq.setData(opts.equity);
    chart.timeScale().fitContent();
  }, [opts.equity, opts.pnlBars]);

  return hostRef;
}

export function TradeDashboardPanel(props: { accessToken: string | null }) {
  const { t } = useTranslation();
  const [ledger, setLedger] = useState<PnlLedger>("live");
  const [bucket, setBucket] = useState<PnlBucket>("daily");
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");
  const [rollups, setRollups] = useState<PnlRollupRowApi[] | null>(null);
  const [equityPoints, setEquityPoints] = useState<PnlEquityPointApi[] | null>(null);
  const [instantOrdersCount, setInstantOrdersCount] = useState<number>(0);
  const [instantSeries, setInstantSeries] = useState<{ tradeCount: MetricPoint[]; netPnl: MetricPoint[] } | null>(null);
  const [exchange, setExchange] = useState("");
  const [segment, setSegment] = useState("");
  const [symbol, setSymbol] = useState("");

  const token = props.accessToken;

  useEffect(() => {
    let cancelled = false;
    async function run() {
      setErr("");
      setRollups(null);
      setEquityPoints(null);
      setInstantSeries(null);
      setInstantOrdersCount(0);
      if (!token) return;
      setLoading(true);
      try {
        if (bucket === "instant") {
          const now = Date.now();
          const since = new Date(now - 24 * 60 * 60_000).toISOString();
          const orders = await fetchMyBinanceOrders(token, { limit: 500, sinceRfc3339: since });
          if (cancelled) return;
          setInstantOrdersCount(orders.length);
          const byMinute = new Map<number, { tradeCount: number; fee: number }>();
          for (const o of orders) {
            const tMs = Date.parse(o.updated_at);
            if (!Number.isFinite(tMs)) continue;
            const k = floorToMinuteUtc(tMs);
            const cur = byMinute.get(k) ?? { tradeCount: 0, fee: 0 };
            const vr = o.venue_response as any;
            const status = typeof vr?.status === "string" ? String(vr.status) : "";
            const isClosed = status === "FILLED" || status === "PARTIALLY_FILLED";
            if (isClosed) {
              cur.tradeCount += 1;
              const fills = Array.isArray(vr?.fills) ? vr.fills : [];
              for (const f of fills) {
                const c = Number(f?.commission);
                if (Number.isFinite(c)) cur.fee += c;
              }
            }
            byMinute.set(k, cur);
          }
          const tradeCount: MetricPoint[] = [];
          const netPnl: MetricPoint[] = [];
          for (const [tMs, v] of byMinute) {
            const time = toUtcTimestampSeconds(tMs);
            tradeCount.push({ time, value: v.tradeCount });
            netPnl.push({ time, value: -v.fee });
          }
          setInstantSeries({ tradeCount: strictAsc(tradeCount), netPnl: strictAsc(netPnl) });
          return;
        }

        const rows = await fetchDashboardPnlRollups(token, ledger, bucket, { exchange, segment, symbol, limit: 800 });
        const eq = await fetchDashboardPnlEquityCurve(token, ledger, bucket, {
          exchange,
          segment,
          symbol,
          limit: 800,
        });
        if (cancelled) return;
        setRollups(rows);
        setEquityPoints(eq);
      } catch (e) {
        if (cancelled) return;
        setErr(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    run();
    return () => {
      cancelled = true;
    };
  }, [token, ledger, bucket, exchange, segment, symbol]);

  const derived = useMemo(() => {
    if (bucket === "instant") {
      const tradeCount = instantSeries?.tradeCount ?? [];
      const netPnl = instantSeries?.netPnl ?? [];
      return { tradeCount, closedTrades: tradeCount, netPnl };
    }
    return rollups ? rollupsToSeries(rollups) : { tradeCount: [], closedTrades: [], netPnl: [] };
  }, [bucket, rollups, instantSeries]);

  const equityDerived = useMemo(() => {
    if (bucket === "instant") return { equity: [], periodNet: [] as HistPoint[] };
    return equityPoints ? equityToSeries(equityPoints) : { equity: [], periodNet: [] as HistPoint[] };
  }, [bucket, equityPoints]);

  const tradeCountChartRef = useMiniChart({ points: derived.tradeCount, accent: "#60a5fa" });
  const closedTradesChartRef = useMiniChart({ points: derived.closedTrades, accent: "#34d399" });
  const netPnlChartRef = useMiniChart({ points: derived.netPnl, accent: "#fb7185", isMoney: true });
  const equityChartRef = useEquityChart({ equity: equityDerived.equity, pnlBars: equityDerived.periodNet });

  if (!token) {
    return (
      <div className="card">
        <p className="tv-drawer__section-head">{t("dashboard.title")}</p>
        <p className="muted" style={{ margin: 0 }}>
          {t("dashboard.signInPrompt")}
        </p>
      </div>
    );
  }

  return (
    <div className="tv-dash">
      <div className="tv-dash__head">
        <div>
          <p className="tv-drawer__section-head" style={{ marginBottom: "0.25rem" }}>
            {t("dashboard.title")}
          </p>
          <p className="muted" style={{ margin: 0, fontSize: "0.78rem" }}>
            {bucket === "instant" ? t("dashboard.instantHint", { n: instantOrdersCount }) : t("dashboard.rollupHint")}
          </p>
        </div>
        <div className="tv-dash__controls">
          <label className="tv-dash__label">
            <span>exchange</span>
            <input
              value={exchange}
              onChange={(e) => setExchange(e.target.value)}
              placeholder="(all)"
              disabled={bucket === "instant"}
            />
          </label>
          <label className="tv-dash__label">
            <span>segment</span>
            <input
              value={segment}
              onChange={(e) => setSegment(e.target.value)}
              placeholder="spot / futures"
              disabled={bucket === "instant"}
            />
          </label>
          <label className="tv-dash__label">
            <span>symbol</span>
            <input
              value={symbol}
              onChange={(e) => setSymbol(e.target.value)}
              placeholder="(all)"
              disabled={bucket === "instant"}
            />
          </label>
          <label className="tv-dash__label">
            <span>{t("dashboard.ledger")}</span>
            <select value={ledger} onChange={(e) => setLedger(e.target.value as PnlLedger)} disabled={bucket === "instant"}>
              <option value="live">{t("dashboard.ledgerLive")}</option>
              <option value="dry">{t("dashboard.ledgerDry")}</option>
            </select>
          </label>
          <div className="tv-dash__seg" role="tablist" aria-label={t("dashboard.bucketAria")}>
            {(["instant", "daily", "weekly", "monthly", "yearly"] as const).map((b) => (
              <button
                key={b}
                type="button"
                role="tab"
                aria-selected={bucket === b}
                className={`tv-dash__segbtn ${bucket === b ? "is-active" : ""}`}
                onClick={() => setBucket(b)}
              >
                {t(`dashboard.bucket.${b}`)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {err ? (
        <div className="card">
          <p className="err" style={{ margin: 0 }}>
            {err}
          </p>
        </div>
      ) : null}

      <div className="tv-dash__grid">
        <div className="tv-dash__card tv-dash__card--wide">
          <div className="tv-dash__cardhead">
            <span className="tv-dash__kpi-title">Equity curve</span>
            <span className="muted" style={{ fontSize: "0.75rem" }}>
              (cum net = realized_pnl − fees)
            </span>
          </div>
          <div ref={equityChartRef} className="tv-dash__chart" />
        </div>
        <div className="tv-dash__card">
          <div className="tv-dash__cardhead">
            <span className="tv-dash__kpi-title">{t("dashboard.metrics.tradeCount")}</span>
          </div>
          <div ref={tradeCountChartRef} className="tv-dash__chart" />
        </div>
        <div className="tv-dash__card">
          <div className="tv-dash__cardhead">
            <span className="tv-dash__kpi-title">{t("dashboard.metrics.closedTrades")}</span>
          </div>
          <div ref={closedTradesChartRef} className="tv-dash__chart" />
        </div>
        <div className="tv-dash__card tv-dash__card--wide">
          <div className="tv-dash__cardhead">
            <span className="tv-dash__kpi-title">{t("dashboard.metrics.netPnl")}</span>
            <span className="muted" style={{ fontSize: "0.75rem" }}>
              {t("dashboard.metrics.netPnlHint")}
            </span>
          </div>
          <div ref={netPnlChartRef} className="tv-dash__chart" />
        </div>
      </div>

      {loading ? (
        <p className="muted" style={{ margin: "0.25rem 0 0" }}>
          {t("dashboard.loading")}
        </p>
      ) : null}
    </div>
  );
}

