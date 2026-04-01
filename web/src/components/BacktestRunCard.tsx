import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ColorType, createChart, type IChartApi, type ISeriesApi, type UTCTimestamp } from "lightweight-charts";
import { backfillMarketBarsFromRest, postBacktestRun, type BacktestRunResponseApi } from "../api/client";

type Props = {
  accessToken: string | null;
  allowBackfill?: boolean;
  defaultExchange: string;
  defaultSegment: string;
  defaultSymbol: string;
  defaultInterval: string;
};

type BacktestStrategyId = "buy_and_hold" | "sma_cross" | "trading_range";

type EquityPoint = { time: UTCTimestamp; value: number };

function isoFromLocalInput(s: string): string {
  // `datetime-local` returns "YYYY-MM-DDTHH:mm" (no tz). Treat as local and convert to ISO.
  const d = new Date(s);
  return Number.isFinite(d.getTime()) ? d.toISOString() : "";
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

function intervalToSeconds(raw: string): number | null {
  const s = (raw ?? "").trim().toLowerCase();
  if (!s) return null;
  const m = s.match(/^(\d+)\s*([mhdw])$/);
  if (!m) return null;
  const n = Number(m[1]);
  if (!Number.isFinite(n) || n <= 0) return null;
  const unit = m[2];
  const mul =
    unit === "m" ? 60 :
    unit === "h" ? 60 * 60 :
    unit === "d" ? 24 * 60 * 60 :
    unit === "w" ? 7 * 24 * 60 * 60 :
    null;
  return mul ? n * mul : null;
}

function estimateBarsBetweenIso(startIso: string, endIso: string, intervalSec: number): number | null {
  const a = new Date(startIso).getTime();
  const b = new Date(endIso).getTime();
  if (!Number.isFinite(a) || !Number.isFinite(b) || intervalSec <= 0) return null;
  if (b < a) return null;
  const spanSec = Math.max(0, Math.floor((b - a) / 1000));
  return Math.floor(spanSec / intervalSec) + 1;
}

export function BacktestRunCard({
  accessToken,
  allowBackfill = false,
  defaultExchange,
  defaultSegment,
  defaultSymbol,
  defaultInterval,
}: Props) {
  const { t } = useTranslation();
  const [strategy, setStrategy] = useState<BacktestStrategyId>("sma_cross");
  const [smaFast, setSmaFast] = useState("10");
  const [smaSlow, setSmaSlow] = useState("30");
  const [exchange, setExchange] = useState(defaultExchange);
  const [segment, setSegment] = useState(defaultSegment);
  const [symbol, setSymbol] = useState(defaultSymbol);
  const [interval, setInterval] = useState(defaultInterval);
  const [startLocal, setStartLocal] = useState("");
  const [endLocal, setEndLocal] = useState("");
  const [initialEquity, setInitialEquity] = useState("100000");
  const [busy, setBusy] = useState(false);
  const [backfillBusy, setBackfillBusy] = useState(false);
  const [err, setErr] = useState("");
  const [res, setRes] = useState<BacktestRunResponseApi | null>(null);
  const [lastRunBody, setLastRunBody] = useState<any>(null);
  const [backfillNote, setBackfillNote] = useState("");

  const wrapRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Line"> | null>(null);

  const equitySeriesData = useMemo((): EquityPoint[] => {
    const rows = res?.equity_curve ?? [];
    const out: EquityPoint[] = [];
    for (const r of rows) {
      const t = Math.floor(new Date(r.ts).getTime() / 1000);
      const v = Number(r.equity);
      if (!Number.isFinite(t) || !Number.isFinite(v)) continue;
      out.push({ time: t as UTCTimestamp, value: v });
    }
    out.sort((a, b) => (a.time as number) - (b.time as number));
    // Dedup same-second.
    const dedup: EquityPoint[] = [];
    for (const p of out) {
      const last = dedup[dedup.length - 1];
      if (!last) dedup.push(p);
      else if ((p.time as number) > (last.time as number)) dedup.push(p);
      else dedup[dedup.length - 1] = p;
    }
    return dedup;
  }, [res]);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const chart = createChart(el, {
      height: 220,
      layout: {
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: "#9a948a",
      },
      rightPriceScale: { borderVisible: false },
      timeScale: { borderVisible: false, timeVisible: true, secondsVisible: false },
      grid: { vertLines: { color: "rgba(0,0,0,0)" }, horzLines: { color: "rgba(0,0,0,0)" } },
    });
    const s = chart.addLineSeries({
      color: "#26a69a",
      lineWidth: 2,
      priceLineVisible: false,
      lastValueVisible: false,
    });
    chartRef.current = chart;
    seriesRef.current = s;

    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      chart.resize(Math.floor(r.width), Math.floor(r.height));
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  useEffect(() => {
    const s = seriesRef.current;
    const c = chartRef.current;
    if (!s || !c) return;
    s.setData(equitySeriesData);
    if (equitySeriesData.length) c.timeScale().fitContent();
  }, [equitySeriesData]);

  const onRun = async () => {
    setErr("");
    setRes(null);
    setBackfillNote("");
    if (!accessToken) {
      setErr("Giriş gerekli (JWT).");
      return;
    }
    const start_time = isoFromLocalInput(startLocal);
    const end_time = isoFromLocalInput(endLocal);
    if (!start_time || !end_time) {
      setErr("Tarih aralığı gerekli.");
      return;
    }
    const ie = Number(initialEquity);
    if (!Number.isFinite(ie) || ie <= 0) {
      setErr("Başlangıç bakiye geçersiz.");
      return;
    }
    setBusy(true);
    try {
      const fastN = Number(smaFast);
      const slowN = Number(smaSlow);
      if (strategy === "sma_cross") {
        if (!Number.isFinite(fastN) || !Number.isFinite(slowN)) {
          setErr("SMA parametreleri geçersiz.");
          return;
        }
        if (Math.floor(fastN) < 1 || Math.floor(slowN) < 2 || Math.floor(fastN) >= Math.floor(slowN)) {
          setErr("SMA için sma_fast < sma_slow olmalı (fast>=1, slow>=2).");
          return;
        }
      }
      const body = {
        strategy,
        exchange: exchange.trim().toLowerCase(),
        segment: segment.trim().toLowerCase(),
        symbol: symbol.trim().toUpperCase(),
        interval: interval.trim(),
        start_time,
        end_time,
        initial_equity: clamp(ie, 1, 1_000_000_000),
        ...(strategy === "sma_cross"
          ? { sma_fast: Math.floor(fastN), sma_slow: Math.floor(slowN) }
          : {}),
      };
      setLastRunBody(body);
      const out = await postBacktestRun(accessToken, body);
      setRes(out);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setErr(msg);
    } finally {
      setBusy(false);
    }
  };

  const onBackfill = async () => {
    setErr("");
    setBackfillNote("");
    if (!accessToken) {
      setErr("Giriş gerekli (JWT).");
      return;
    }
    if (!allowBackfill) {
      setErr("Backfill için ops rolü gerekli (trader/admin).");
      return;
    }
    const seg = segment.trim().toLowerCase();
    const segApi = seg.includes("future") || seg === "fapi" ? "futures" : "spot";
    const sym = symbol.trim().toUpperCase();
    const iv = interval.trim();
    if (!sym || !iv) {
      setErr("Symbol/interval boş olamaz.");
      return;
    }
    const startIso = isoFromLocalInput(startLocal);
    const endIso = isoFromLocalInput(endLocal);
    const ivSec = intervalToSeconds(iv);
    const estBars =
      startIso && endIso && ivSec
        ? estimateBarsBetweenIso(startIso, endIso, ivSec)
        : null;
    const planned = clamp(estBars ?? 2000, 10, 50_000);
    setBackfillBusy(true);
    try {
      const r = await backfillMarketBarsFromRest(accessToken, {
        symbol: sym,
        interval: iv,
        segment: segApi,
        limit: planned,
      });
      setBackfillNote(`Backfill ok: upserted=${r.upserted ?? "?"} · segment=${r.segment ?? segApi} · source=${r.source ?? "?"}`);
      // If the user already tried to run, re-run with the same parameters.
      if (lastRunBody) {
        const out = await postBacktestRun(accessToken, lastRunBody);
        setRes(out);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBackfillBusy(false);
    }
  };

  return (
    <div className="card">
      <p className="tv-drawer__section-head">Backtest</p>
      <div className="tv-settings__fields">
        <label className="muted">
          <span>Strategy</span>
          <select
            className="tv-topstrip__select"
            value={strategy}
            onChange={(e) => setStrategy(e.target.value as BacktestStrategyId)}
          >
            <option value="sma_cross">SMA cross (10/30)</option>
            <option value="buy_and_hold">Buy & hold</option>
            <option value="trading_range">Trading range (signal dashboard)</option>
          </select>
        </label>
        {strategy === "trading_range" ? (
          <p className="muted" style={{ fontSize: "0.72rem", margin: "-0.2rem 0 0.15rem", lineHeight: 1.45 }}>
            {t("app.backtest.tradingRangeServerNote")}
          </p>
        ) : null}
        {strategy === "sma_cross" ? (
          <>
            <label className="muted">
              <span>sma_fast</span>
              <input className="tv-topstrip__input mono" value={smaFast} onChange={(e) => setSmaFast(e.target.value)} />
            </label>
            <label className="muted">
              <span>sma_slow</span>
              <input className="tv-topstrip__input mono" value={smaSlow} onChange={(e) => setSmaSlow(e.target.value)} />
            </label>
          </>
        ) : null}
        <label className="muted">
          <span>Exchange</span>
          <input className="tv-topstrip__input mono" value={exchange} onChange={(e) => setExchange(e.target.value)} />
        </label>
        <label className="muted">
          <span>Segment</span>
          <input className="tv-topstrip__input mono" value={segment} onChange={(e) => setSegment(e.target.value)} />
        </label>
        <label className="muted">
          <span>Symbol</span>
          <input className="tv-topstrip__input mono" value={symbol} onChange={(e) => setSymbol(e.target.value.toUpperCase())} />
        </label>
        <label className="muted">
          <span>Interval</span>
          <input className="tv-topstrip__input mono" value={interval} onChange={(e) => setInterval(e.target.value)} />
        </label>
        <label className="muted">
          <span>Start</span>
          <input className="tv-topstrip__input mono" type="datetime-local" value={startLocal} onChange={(e) => setStartLocal(e.target.value)} />
        </label>
        <label className="muted">
          <span>End</span>
          <input className="tv-topstrip__input mono" type="datetime-local" value={endLocal} onChange={(e) => setEndLocal(e.target.value)} />
        </label>
        <label className="muted">
          <span>Initial equity</span>
          <input className="tv-topstrip__input mono" value={initialEquity} onChange={(e) => setInitialEquity(e.target.value)} />
        </label>
      </div>

      <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.6rem", alignItems: "center", flexWrap: "wrap" }}>
        <button type="button" className="theme-toggle" onClick={onRun} disabled={busy}>
          {busy ? "Çalışıyor…" : "Run"}
        </button>
        <button
          type="button"
          className="theme-toggle"
          onClick={onBackfill}
          disabled={backfillBusy || busy || !accessToken}
          title={allowBackfill ? "Binance REST → market_bars backfill (ops)" : "Ops rolü gerekli"}
        >
          {backfillBusy ? "Backfill…" : "Backfill (Binance→DB)"}
        </button>
        <span className="muted" style={{ fontSize: "0.8rem" }}>
          API: <code>POST /api/v1/backtest/run</code>
        </span>
      </div>

      {backfillNote ? <p className="muted mono" style={{ marginTop: "0.45rem" }}>{backfillNote}</p> : null}
      {err ? <p className="err" style={{ marginTop: "0.45rem" }}>{err}</p> : null}
      {err &&
      strategy === "trading_range" &&
      /buy_and_hold/i.test(err) &&
      /sma_cross/i.test(err) &&
      !/trading_range/i.test(err) ? (
        <p className="muted" style={{ marginTop: "0.35rem", fontSize: "0.78rem", lineHeight: 1.45 }}>
          {t("app.backtest.oldApiStrategyErrorHint")}
        </p>
      ) : null}
      {err && /yeterli veri yok|market_bars/i.test(err) ? (
        <p className="muted" style={{ marginTop: "0.25rem", fontSize: "0.8rem" }}>
          İpucu: Önce <strong>Backfill (Binance→DB)</strong> çalıştırın, ardından <strong>Run</strong>.
        </p>
      ) : null}

      {res?.report ? (
        <div style={{ marginTop: "0.55rem" }}>
          <p className="muted" style={{ margin: 0, fontSize: "0.8rem" }}>
            Report:{" "}
            <span className="mono">
              {(() => {
                const r = res.report as any;
                const tr = r?.total_return;
                const dd = r?.max_drawdown;
                const pf = r?.profit_factor;
                const wr = r?.win_rate;
                const n = res.trades?.length ?? 0;
                const parts = [
                  tr != null ? `return=${String(tr)}` : null,
                  dd != null ? `maxDD=${String(dd)}` : null,
                  pf != null ? `PF=${String(pf)}` : null,
                  wr != null ? `winRate=${String(wr)}` : null,
                  `trades=${n}`,
                ].filter(Boolean);
                return parts.join(" · ");
              })()}
            </span>
          </p>
        </div>
      ) : null}

      {res?.meta ? (
        <p className="muted mono" style={{ marginTop: "0.25rem", fontSize: "0.78rem" }}>
          meta: bars={res.meta.bar_count} · {res.meta.exchange}/{res.meta.segment} · {res.meta.symbol} {res.meta.interval} · strategy={res.meta.strategy}
        </p>
      ) : null}

      <div style={{ marginTop: "0.75rem" }}>
        <div ref={wrapRef} style={{ width: "100%", height: "220px" }} aria-label="Equity curve chart" />
      </div>

      {res?.trades?.length ? (
        <div style={{ marginTop: "0.75rem", overflowX: "auto" }}>
          <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Entry</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Exit</th>
                <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Side</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>Qty</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>EntryPx</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>ExitPx</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>Fee</th>
                <th style={{ textAlign: "right", padding: "0.15rem 0.2rem" }}>PnL</th>
              </tr>
            </thead>
            <tbody>
              {res.trades.map((t, i) => (
                <tr key={i}>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{new Date(t.entry_ts).toISOString()}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{new Date(t.exit_ts).toISOString()}</td>
                  <td style={{ padding: "0.12rem 0.2rem" }}>{t.side}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{t.qty}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{t.entry_px}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{t.exit_px}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{t.fee}</td>
                  <td style={{ padding: "0.12rem 0.2rem", textAlign: "right" }}>{t.pnl}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p className="muted" style={{ marginTop: "0.65rem", fontSize: "0.8rem" }}>
          Trade list burada görünecek.
        </p>
      )}
    </div>
  );
}

