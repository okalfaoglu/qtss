// QTSS RADAR periodic performance reports (Faz 20B).
//
// Single page showing the active performance snapshot per
// market × period × mode. Mirrors the QTSS RADAR screenshot template:
// KAPANIŞLAR (closings table), SERMAYE TAKİBİ (capital tracking),
// PERFORMANS + RİSK metric panels, RISK-ON/OFF indicator.
//
// Data: /v2/radar/{market}/{period}?mode=<live|dry|backtest>.
// The aggregator worker refreshes the underlying table every 5 min.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

type Period = "live" | "daily" | "weekly" | "monthly" | "yearly";
type Market = "coin";
type Mode = "live" | "dry" | "backtest";

// /v2/radar/live/{market} response — open-trade snapshot for the
// "Anlık" tab. Distinct from closed-trade aggregation (RadarReport)
// because the semantics are different: one is "right now", the other
// "over a period". Rendering chooses the shape from `period`.
interface LiveTrade {
  setup_id: string | null;
  symbol: string;
  timeframe: string;
  direction: string;
  mode: string;
  entry_price: number;
  /** Backlog #4 — original setup entry level (the price the system
      armed at) vs entry_price which is the actual fill (entry_avg
      from live_positions). The drawer renders both so the operator
      can see slippage + market-impact divergence. */
  setup_entry_price?: number | null;
  mark_price: number | null;
  qty: number;
  notional_usd: number;
  sl: number | null;
  tp: number | null;
  tp_ladder: Array<{
    idx?: number;
    price?: number;
    size_pct?: number;
    qty?: number;
  }>;
  u_pnl_pct: number | null;
  u_pnl_usd: number | null;
  opened_at: string;
  leverage: number;
  profile: string | null;
}

interface LiveResponse {
  market: string;
  mode: string;
  computed_at: string;
  open_count: number;
  long_count: number;
  short_count: number;
  open_exposure_usd: number;
  avg_u_pnl_pct: number | null;
  sum_u_pnl_usd: number;
  starting_capital_usd: number;
  current_equity_usd: number;
  cash_position_pct: number;
  distinct_symbols: number;
  trades: LiveTrade[];
}

interface RadarTrade {
  symbol: string;
  direction: string;
  date: string;
  notional_usd: number;
  pnl_usd: number;
  return_pct: number;
  status: string;
  // FAZ 25 — backend includes the setup profile so the report can
  // pivot legacy T/D vs IQ-D/IQ-T performance. Field is optional
  // because old / running deployments may not surface it yet.
  profile?: string | null;
}

interface RadarReport {
  market: string;
  period: string;
  mode: string;
  period_start: string;
  period_end: string;
  finalised: boolean;
  closed_count: number;
  win_count: number;
  loss_count: number;
  win_rate: number | null;
  total_notional_usd: number | null;
  total_pnl_usd: number | null;
  avg_return_pct: number | null;
  compound_return_pct: number | null;
  avg_allocation_usd: number | null;
  avg_holding_bars: number | null;
  starting_capital_usd: number | null;
  ending_capital_usd: number | null;
  cash_position_pct: number | null;
  open_exposure_usd: number | null;
  risk_mode: string | null;
  volatility_level: string | null;
  max_position_risk_pct: number | null;
  trades: RadarTrade[] | null;
  computed_at: string;
}

interface RadarResponse {
  market: string;
  period: string;
  latest: RadarReport | null;
  history: RadarReport[];
}

const PERIODS: Array<{ key: Period; label: string }> = [
  // "Anlık" — open-trade snapshot (live_positions). Different data
  // shape than the aggregated periods, so its own fetch + render path.
  { key: "live", label: "Anlık" },
  { key: "daily", label: "Günlük" },
  { key: "weekly", label: "Haftalık" },
  { key: "monthly", label: "Aylık" },
  { key: "yearly", label: "Yıllık" },
];

const MARKETS: Array<{ key: Market; label: string }> = [
  { key: "coin", label: "Coin" },
];

const MODES: Array<{ key: Mode; label: string }> = [
  { key: "live", label: "Live" },
  { key: "dry", label: "Dry" },
  { key: "backtest", label: "Backtest" },
];

function fmtUsd(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  if (Math.abs(n) >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (Math.abs(n) >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(2);
}

function fmtPct(n: number | null | undefined, digits = 2): string {
  if (n === null || n === undefined) return "—";
  return `${n >= 0 ? "+" : ""}${n.toFixed(digits)}%`;
}

function signClass(n: number | null | undefined): string {
  if (n === null || n === undefined) return "text-zinc-400";
  if (n > 0) return "text-emerald-400";
  if (n < 0) return "text-rose-400";
  return "text-zinc-400";
}

// Convert holding-in-bars to a human-readable duration band. Without the
// per-trade timeframe we infer a 15-minute bar (the dominant TF in the
// live allocator) and bucket into days. Good enough for operator glance;
// the exact figure is still retrievable from `avg_holding_bars`.
function formatHolding(bars: number | null, period: string | null): string {
  if (bars == null) return "—";
  // 15-min bars → 96/day; for weekly/monthly reports upscale assumption
  // so multi-day holds read cleanly ("1-2 GÜN" instead of "96 bar").
  const perDay = 96;
  const days = bars / perDay;
  if (days < 0.04) return "< 1 SAAT";
  if (days < 0.25) return "SAATLİK";
  if (days < 1) return "GÜN İÇİ";
  if (days < 2) return "1-2 GÜN";
  if (days < 7) return `${days.toFixed(0)} GÜN`;
  if (days < 30) return `${(days / 7).toFixed(1)} HAFTA`;
  // fall back to raw if the bucket is atypical
  void period;
  return `${bars.toFixed(1)} bar`;
}

// Tally distinct symbols in the closed trade list and map to a band.
// Keeps the rendering logic in the UI since the aggregator already
// ships `trades` and this is display-only.
function distinctSymbols(trades: RadarTrade[] | null | undefined): number {
  if (!trades || trades.length === 0) return 0;
  const s = new Set<string>();
  for (const t of trades) if (t.symbol) s.add(t.symbol);
  return s.size;
}

function portfolioSpread(trades: RadarTrade[] | null | undefined): string {
  const n = distinctSymbols(trades);
  if (n === 0) return "—";
  if (n === 1) return "YOĞUN";
  if (n <= 3) return "ORTA";
  return "DENGELİ";
}

function correlationRisk(trades: RadarTrade[] | null | undefined): string {
  const n = distinctSymbols(trades);
  if (n === 0) return "—";
  if (n <= 1) return "YÜKSEK";
  if (n <= 3) return "ORTA";
  return "DÜŞÜK";
}

function RiskModeBadge({ mode }: { mode: string | null }) {
  if (!mode) return <span className="text-zinc-500">—</span>;
  const palette: Record<string, string> = {
    risk_on: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
    neutral: "bg-amber-500/20 text-amber-300 border-amber-500/40",
    risk_off: "bg-rose-500/20 text-rose-300 border-rose-500/40",
  };
  const label: Record<string, string> = {
    risk_on: "RISK-ON",
    neutral: "NÖTR",
    risk_off: "RISK-OFF",
  };
  return (
    <span
      className={`inline-flex items-center rounded border px-3 py-1 text-xs font-mono uppercase tracking-wider ${
        palette[mode] ?? "bg-zinc-700 text-zinc-200"
      }`}
    >
      {label[mode] ?? mode}
    </span>
  );
}

export default function RadarReports() {
  const [period, setPeriod] = useState<Period>("weekly");
  const [market, setMarket] = useState<Market>("coin");
  const [mode, setMode] = useState<Mode>("dry");
  // FAZ 25 — profile filter so the report can split legacy T/D
  // performance from the new IQ-D / IQ-T pipeline. Default "all"
  // matches the previous behaviour (no filter).
  type ProfileFilter = "all" | "legacy" | "iq" | "iq_d" | "iq_t" | "t" | "d";
  const [profileFilter, setProfileFilter] = useState<ProfileFilter>("all");
  const matchProfile = (p?: string | null): boolean => {
    if (profileFilter === "all") return true;
    const profile = (p ?? "").toLowerCase();
    if (profileFilter === "legacy") return ["t", "d", "q"].includes(profile);
    if (profileFilter === "iq") return profile.startsWith("iq_");
    return profile === profileFilter;
  };

  const isLive = period === "live";

  // Closed-trade aggregate (period = daily/weekly/monthly/yearly).
  // Skipped when the Anlık tab is active to avoid an extra round trip.
  const q = useQuery<RadarResponse>({
    queryKey: ["radar", market, period, mode],
    queryFn: () =>
      apiFetch(`/v2/radar/${market}/${period}?mode=${mode}&history=true`),
    refetchInterval: 60_000,
    enabled: !isLive,
  });

  // Open-trade snapshot (period = live). 10-second refetch so u-PnL
  // tracks the bookTicker mark without hammering the backend.
  const qLive = useQuery<LiveResponse>({
    queryKey: ["radar-live", market, mode],
    queryFn: () => apiFetch(`/v2/radar/live/${market}?mode=${mode}`),
    refetchInterval: 10_000,
    enabled: isLive,
  });

  const latest = q.data?.latest ?? null;
  const allTrades = latest?.trades ?? [];
  const trades = allTrades.filter((t) => matchProfile(t.profile));

  return (
    <div className="flex h-full flex-col gap-3 p-4 font-mono text-sm text-zinc-200">
      {/* Header */}
      <div className="flex items-baseline gap-4 border-b border-zinc-800 pb-3">
        <div>
          <span className="text-xl font-bold tracking-wider text-zinc-100">
            QTSS <span className="text-emerald-400">RADAR</span>
          </span>
          <span className="ml-3 text-xs uppercase tracking-widest text-zinc-500">
            {PERIODS.find((p) => p.key === period)?.label} Rapor —{" "}
            {MARKETS.find((m) => m.key === market)?.label}
          </span>
        </div>
        <div className="ml-auto text-xs text-zinc-500">
          {latest?.computed_at
            ? `son güncelleme: ${new Date(latest.computed_at).toLocaleString()}`
            : ""}
        </div>
      </div>

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-zinc-500">
          period
        </span>
        {PERIODS.map((p) => (
          <button
            key={p.key}
            type="button"
            onClick={() => setPeriod(p.key)}
            className={`rounded border px-3 py-1 text-xs ${
              period === p.key
                ? "border-emerald-500 bg-emerald-500/20 text-emerald-300"
                : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-500"
            }`}
          >
            {p.label}
          </button>
        ))}
        <span className="ml-4 text-[10px] uppercase tracking-wider text-zinc-500">
          market
        </span>
        {MARKETS.map((m) => (
          <button
            key={m.key}
            type="button"
            onClick={() => setMarket(m.key)}
            className={`rounded border px-3 py-1 text-xs ${
              market === m.key
                ? "border-emerald-500 bg-emerald-500/20 text-emerald-300"
                : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-500"
            }`}
          >
            {m.label}
          </button>
        ))}
        <span className="text-[10px] uppercase tracking-wider text-zinc-600">
          (BIST + NASDAQ işlemler başlayınca otomatik eklenecek)
        </span>
        <span className="ml-4 text-[10px] uppercase tracking-wider text-zinc-500">
          mode
        </span>
        {MODES.map((m) => (
          <button
            key={m.key}
            type="button"
            onClick={() => setMode(m.key)}
            className={`rounded border px-3 py-1 text-xs ${
              mode === m.key
                ? "border-sky-500 bg-sky-500/20 text-sky-300"
                : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-500"
            }`}
          >
            {m.label}
          </button>
        ))}
        {/* FAZ 25 — profile pivot. Lets the operator split legacy
            T / D performance from the new IQ-D / IQ-T pipeline. */}
        <span className="ml-4 text-[10px] uppercase tracking-wider text-zinc-500">
          profil
        </span>
        {([
          { key: "all", label: "Hepsi" },
          { key: "legacy", label: "T+D" },
          { key: "t", label: "T" },
          { key: "d", label: "D" },
          { key: "iq", label: "IQ" },
          { key: "iq_d", label: "IQ-D" },
          { key: "iq_t", label: "IQ-T" },
        ] as Array<{ key: ProfileFilter; label: string }>).map((p) => (
          <button
            key={p.key}
            type="button"
            onClick={() => setProfileFilter(p.key)}
            className={`rounded border px-3 py-1 text-xs ${
              profileFilter === p.key
                ? "border-indigo-500 bg-indigo-500/20 text-indigo-200"
                : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-500"
            }`}
          >
            {p.label}
          </button>
        ))}
      </div>

      {!isLive && q.isLoading && <div className="text-zinc-500">yükleniyor…</div>}
      {!isLive && q.isError && (
        <div className="text-rose-400">
          hata: {(q.error as Error)?.message ?? "rapor çekilemedi"}
        </div>
      )}
      {isLive && qLive.isLoading && (
        <div className="text-zinc-500">yükleniyor…</div>
      )}
      {isLive && qLive.isError && (
        <div className="text-rose-400">
          hata: {(qLive.error as Error)?.message ?? "anlık veri çekilemedi"}
        </div>
      )}

      {isLive && qLive.data && (
        <LivePanel
          data={{
            ...qLive.data,
            // Apply the profile pivot to live snapshots too — the
            // backend doesn't know about it, so we filter client-
            // side. Aggregate KPIs (total PnL, win rate) downstream
            // recompute over the filtered list.
            trades: (qLive.data.trades ?? []).filter((t) =>
              matchProfile(t.profile),
            ),
          }}
        />
      )}

      {!isLive && latest && (
        <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
          {/* KAPANIŞLAR */}
          <div className="col-span-2 rounded border border-zinc-800 bg-zinc-950/40">
            <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-emerald-400">
              ■ Kapanan Analizler
            </div>
            {trades.length === 0 ? (
              <div className="p-4 text-xs text-zinc-500">
                Bu dönemde kapanmış işlem yok.
              </div>
            ) : (
              <table className="w-full text-xs">
                <thead className="border-b border-zinc-800 text-zinc-500">
                  <tr>
                    <th className="px-3 py-2 text-left">Hisse</th>
                    <th className="px-3 py-2 text-left">Tarih</th>
                    <th className="px-3 py-2 text-right">Bağlanan ($)</th>
                    <th className="px-3 py-2 text-right">Kazanç ($)</th>
                    <th className="px-3 py-2 text-right">Getiri (%)</th>
                    <th className="px-3 py-2 text-center">Durum</th>
                  </tr>
                </thead>
                <tbody>
                  {trades.map((t, i) => (
                    <tr
                      key={i}
                      className="border-b border-zinc-900 hover:bg-zinc-900/60"
                    >
                      <td className="px-3 py-2 font-semibold">
                        {t.direction === "long" ? "●" : "◆"} {t.symbol}
                      </td>
                      <td className="px-3 py-2 text-zinc-400">
                        {t.date ? new Date(t.date).toLocaleDateString() : "—"}
                      </td>
                      <td className="px-3 py-2 text-right text-zinc-300">
                        {fmtUsd(t.notional_usd)}
                      </td>
                      <td className={`px-3 py-2 text-right ${signClass(t.pnl_usd)}`}>
                        {t.pnl_usd >= 0 ? "+" : ""}
                        {fmtUsd(t.pnl_usd)}
                      </td>
                      <td className={`px-3 py-2 text-right ${signClass(t.return_pct)}`}>
                        {fmtPct(t.return_pct)}
                      </td>
                      <td className="px-3 py-2 text-center">
                        <span
                          className={`inline-block h-2 w-2 rounded-full ${
                            t.status === "win"
                              ? "bg-emerald-500"
                              : t.status === "loss"
                                ? "bg-rose-500"
                                : "bg-zinc-500"
                          }`}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* SERMAYE TAKİBİ */}
          <div className="rounded border border-zinc-800 bg-zinc-950/40">
            <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-emerald-400">
              ■ Sermaye Takibi
            </div>
            <div className="space-y-2 p-3 text-xs">
              <RowKV
                k="Sermaye Başlangıcı"
                v={`${fmtUsd(latest.starting_capital_usd)} $`}
              />
              <RowKV
                k="Toplam Kazanç"
                v={fmtUsd(latest.total_pnl_usd)}
                cls={signClass(latest.total_pnl_usd)}
              />
              <RowKV
                k="Güncel Sermaye"
                v={`${fmtUsd(latest.ending_capital_usd)} $`}
                cls="text-emerald-300"
              />
              <RowKV
                k="Bileşik Getiri"
                v={fmtPct(latest.compound_return_pct)}
                cls={signClass(latest.compound_return_pct)}
              />
              <div className="my-2 border-t border-zinc-800" />
              <RowKV
                k="Toplam Yatırım (Kapanan)"
                v={`${fmtUsd(latest.total_notional_usd)} $`}
              />
              <RowKV
                k="Açık Pozisyon"
                v={`${fmtUsd(latest.open_exposure_usd)} $`}
                cls={
                  (latest.open_exposure_usd ?? 0) > 0
                    ? "text-amber-300"
                    : undefined
                }
              />
              <RowKV
                k="Ort. Allocation"
                v={`${fmtUsd(latest.avg_allocation_usd)} $`}
              />
              <RowKV
                k="Toplam Kapanış"
                v={`${latest.closed_count} işlem (${latest.win_count} kârlı / ${latest.loss_count} zararlı)`}
              />
              <RowKV k="Nakit Pozisyon" v={fmtPct(latest.cash_position_pct)} />
            </div>
          </div>

          {/* Summary strip — matches QTSK RADAR template:
              KAPANIŞ / KÂRLI / BAŞARI / TOPLAM BAĞLANAN / TOPLAM KAZANÇ /
              ORT. POZ. GETİRİSİ / İŞLEM BAŞINA ORT. BAĞLANAN */}
          <div className="col-span-3 grid grid-cols-7 gap-2">
            <StatCell label="Kapanış" value={`${latest.closed_count}`} />
            <StatCell
              label="Kârlı"
              value={`${latest.win_count}/${latest.closed_count}`}
            />
            <StatCell
              label="Başarı"
              value={fmtPct((latest.win_rate ?? 0) * 100, 1)}
              cls="text-amber-400"
            />
            <StatCell
              label="Toplam Bağlanan"
              value={`${fmtUsd(latest.total_notional_usd)} $`}
            />
            <StatCell
              label="Toplam Kazanç"
              value={`${fmtUsd(latest.total_pnl_usd)} $`}
              cls={signClass(latest.total_pnl_usd)}
            />
            <StatCell
              label="Ort. Poz. Getirisi"
              value={fmtPct(latest.avg_return_pct)}
              cls={signClass(latest.avg_return_pct)}
            />
            <StatCell
              label="İşlem Başına Ort. Bağlanan"
              value={`${fmtUsd(latest.avg_allocation_usd)} $`}
            />
          </div>

          {/* PERFORMANS + RİSK */}
          <div className="rounded border border-zinc-800 bg-zinc-950/40">
            <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-emerald-400">
              ■ Performans Göstergeleri
            </div>
            <div className="space-y-2 p-3 text-xs">
              <RowKV k="Win Rate" v={fmtPct((latest.win_rate ?? 0) * 100, 2)} />
              <RowKV
                k="Avg Allocation"
                v={`${fmtUsd(latest.avg_allocation_usd)} $`}
              />
              {/* Sermaye kullanımı — kapanan işlem sermayesinin anaparaya oranı. */}
              <RowKV
                k="Sermaye Kullanımı"
                v={fmtPct(
                  latest.starting_capital_usd
                    ? ((latest.total_notional_usd ?? 0) /
                        latest.starting_capital_usd) *
                        100
                    : null,
                  2,
                )}
              />
              <RowKV k="Bileşik Getiri" v={fmtPct(latest.compound_return_pct)} />
              <RowKV k="Ort. Poz. Getirisi" v={fmtPct(latest.avg_return_pct)} />
              <RowKV
                k="Avg. Holding Period"
                v={formatHolding(latest.avg_holding_bars, latest.period)}
              />
            </div>
          </div>

          <div className="rounded border border-zinc-800 bg-zinc-950/40">
            <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-emerald-400">
              ■ Risk Metrikleri
            </div>
            <div className="space-y-2 p-3 text-xs">
              <div className="flex items-center justify-between">
                <span className="text-zinc-500">Risk Modu</span>
                <RiskModeBadge mode={latest.risk_mode} />
              </div>
              <RowKV
                k="Volatilite Seviyesi"
                v={
                  latest.volatility_level
                    ? latest.volatility_level.toUpperCase()
                    : "—"
                }
              />
              {/* Portföy dağılımı — kapanmış işlemlerdeki farklı sembol
                  sayısından türetilir: 1 sembol = YOĞUN, 2-3 = ORTA,
                  4+ = DENGELİ. */}
              <RowKV
                k="Portföy Dağılımı"
                v={portfolioSpread(latest.trades)}
              />
              <RowKV
                k="Maks. Pozisyon Risk"
                v={fmtPct((latest.max_position_risk_pct ?? 0) * 100, 2)}
              />
              {/* Korelasyon riski — aynı mantık farklı ifade: az sembol →
                  yüksek korelasyon riski. */}
              <RowKV
                k="Korelasyon Risk"
                v={correlationRisk(latest.trades)}
              />
              <RowKV k="Nakit Pozisyon" v={fmtPct(latest.cash_position_pct)} />
            </div>
          </div>

          <div className="rounded border border-zinc-800 bg-zinc-950/40 p-3">
            <div className="mb-2 text-[10px] uppercase tracking-wider text-emerald-400">
              ■ Risk Modu
            </div>
            <div className="flex flex-col items-center gap-3 py-4">
              <RiskModeBadge mode={latest.risk_mode} />
              <div className="flex gap-2">
                <span
                  className={`rounded px-2 py-0.5 text-[10px] ${
                    latest.risk_mode === "risk_off"
                      ? "bg-rose-500/30 text-rose-200"
                      : "bg-zinc-800 text-zinc-500"
                  }`}
                >
                  RISK-OFF
                </span>
                <span
                  className={`rounded px-2 py-0.5 text-[10px] ${
                    latest.risk_mode === "neutral"
                      ? "bg-amber-500/30 text-amber-200"
                      : "bg-zinc-800 text-zinc-500"
                  }`}
                >
                  NÖTR
                </span>
                <span
                  className={`rounded px-2 py-0.5 text-[10px] ${
                    latest.risk_mode === "risk_on"
                      ? "bg-emerald-500/30 text-emerald-200"
                      : "bg-zinc-800 text-zinc-500"
                  }`}
                >
                  RISK-ON
                </span>
              </div>
              <div className="mt-3 text-xs uppercase tracking-widest text-zinc-500">
                Nakit Pozisyon
              </div>
              <div
                className={`text-2xl font-bold ${
                  (latest.cash_position_pct ?? 100) >= 90
                    ? "text-emerald-300"
                    : (latest.cash_position_pct ?? 100) >= 40
                      ? "text-amber-300"
                      : "text-rose-300"
                }`}
              >
                {fmtPct(latest.cash_position_pct, 2)}
              </div>
            </div>
          </div>
        </div>
      )}

      {!latest && !q.isLoading && (
        <div className="rounded border border-zinc-800 bg-zinc-950/40 p-8 text-center text-zinc-500">
          Bu dönem için rapor henüz üretilmedi (aggregator her 5 dakikada bir
          çalışır). Lütfen biraz sonra tekrar deneyin.
        </div>
      )}

      <div className="mt-auto border-t border-zinc-800 pt-2 text-center text-[10px] uppercase tracking-widest text-zinc-600">
        QTSS RADAR · discipline · patience · process
      </div>
    </div>
  );
}

function RowKV({
  k,
  v,
  cls,
}: {
  k: string;
  v: string;
  cls?: string;
}) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-zinc-500">{k}</span>
      <span className={cls ?? "text-zinc-200"}>{v}</span>
    </div>
  );
}

function StatCell({
  label,
  value,
  cls,
}: {
  label: string;
  value: string;
  cls?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-1 rounded border border-zinc-800 bg-zinc-950/40 py-3">
      <div className="text-[10px] uppercase tracking-wider text-zinc-500">
        {label}
      </div>
      <div className={`text-lg font-bold ${cls ?? "text-zinc-100"}`}>
        {value}
      </div>
    </div>
  );
}

// ── Anlık (Live) tab ────────────────────────────────────────────────
//
// Renders /v2/radar/live/{market} — the open-trade snapshot. Different
// shape from the aggregated periods so the component is intentionally
// separate rather than trying to shoehorn RadarReport into it.

function LivePanel({ data }: { data: LiveResponse }) {
  const rows = data.trades;
  const [selected, setSelected] = useState<LiveTrade | null>(null);
  const symSet = new Set(rows.map((r) => r.symbol));
  const spread =
    symSet.size === 0
      ? "—"
      : symSet.size === 1
        ? "YOĞUN"
        : symSet.size <= 3
          ? "ORTA"
          : "DENGELİ";
  const corrRisk =
    symSet.size === 0
      ? "—"
      : symSet.size <= 1
        ? "YÜKSEK"
        : symSet.size <= 3
          ? "ORTA"
          : "DÜŞÜK";
  const avgAllocation =
    data.open_count > 0 ? data.open_exposure_usd / data.open_count : 0;
  const capitalUse =
    data.starting_capital_usd > 0
      ? (data.open_exposure_usd / data.starting_capital_usd) * 100
      : 0;
  const sumUpnlPctRough =
    data.starting_capital_usd > 0
      ? (data.sum_u_pnl_usd / data.starting_capital_usd) * 100
      : 0;
  // Risk modu cash-aware: >=90 risk_off, 40-90 neutral, <40 risk_on.
  const riskMode =
    data.cash_position_pct >= 90
      ? "risk_off"
      : data.cash_position_pct >= 40
        ? "neutral"
        : "risk_on";

  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
      {/* AÇIK İŞLEMLER — replaces KAPANAN ANALİZLER */}
      <div className="col-span-2 rounded border border-zinc-800 bg-zinc-950/40">
        <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-amber-300">
          ■ Açık İşlemler
        </div>
        {rows.length === 0 ? (
          <div className="p-4 text-xs text-zinc-500">
            Şu an açık pozisyon yok.
          </div>
        ) : (
          <table className="w-full text-xs">
            <thead className="border-b border-zinc-800 text-zinc-500">
              <tr>
                <th className="px-3 py-2 text-left">Sembol</th>
                <th className="px-3 py-2 text-left">TF</th>
                <th className="px-3 py-2 text-left">Yön</th>
                <th className="px-3 py-2 text-right">Bağlanan ($)</th>
                <th className="px-3 py-2 text-right">u-PnL ($)</th>
                <th className="px-3 py-2 text-right">Getiri (%)</th>
                <th className="px-3 py-2 text-center">Durum</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((t, i) => {
                const isLong = t.direction === "long" || t.direction === "buy";
                const up = (t.u_pnl_pct ?? 0) > 0;
                const down = (t.u_pnl_pct ?? 0) < 0;
                return (
                  <tr
                    key={t.setup_id ?? `${t.symbol}-${i}`}
                    onClick={() => setSelected(t)}
                    className="cursor-pointer border-b border-zinc-900 hover:bg-zinc-900/60"
                    title="detay için tıkla"
                  >
                    <td className="px-3 py-2 font-semibold">
                      {isLong ? "●" : "◆"} {t.symbol}
                      <span className="ml-2 text-[10px] text-zinc-500">
                        {isLong ? "LONG" : "SHORT"}
                      </span>
                      {t.leverage > 1 && (
                        <span className="ml-2 rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] text-amber-300">
                          {t.leverage}x
                        </span>
                      )}
                    </td>
                    <td className="px-3 py-2 text-zinc-400">{t.timeframe}</td>
                    <td className="px-3 py-2 text-zinc-500">
                      {formatAge(t.opened_at)}
                    </td>
                    <td className="px-3 py-2 text-right text-zinc-300">
                      {fmtUsd(t.notional_usd)}
                    </td>
                    <td
                      className={`px-3 py-2 text-right ${signClass(t.u_pnl_usd)}`}
                    >
                      {t.u_pnl_usd != null
                        ? `${t.u_pnl_usd >= 0 ? "+" : ""}${fmtUsd(t.u_pnl_usd)}`
                        : "—"}
                    </td>
                    <td
                      className={`px-3 py-2 text-right ${signClass(t.u_pnl_pct)}`}
                    >
                      {fmtPct(t.u_pnl_pct)}
                    </td>
                    <td className="px-3 py-2 text-center">
                      <span
                        className={`inline-block h-2 w-2 rounded-full ${
                          up
                            ? "bg-emerald-500"
                            : down
                              ? "bg-rose-500"
                              : "bg-zinc-500"
                        }`}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* SERMAYE TAKİBİ (anlık odaklı) */}
      <div className="rounded border border-zinc-800 bg-zinc-950/40">
        <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-amber-300">
          ■ Sermaye Takibi
        </div>
        <div className="space-y-2 p-3 text-xs">
          <RowKV
            k="Sermaye Başlangıcı"
            v={`${fmtUsd(data.starting_capital_usd)} $`}
          />
          <RowKV
            k="Toplam u-PnL"
            v={`${data.sum_u_pnl_usd >= 0 ? "+" : ""}${fmtUsd(data.sum_u_pnl_usd)} $`}
            cls={signClass(data.sum_u_pnl_usd)}
          />
          <RowKV
            k="Güncel Sermaye"
            v={`${fmtUsd(data.current_equity_usd)} $`}
            cls="text-emerald-300"
          />
          <RowKV
            k="Bileşik Getiri (u)"
            v={fmtPct(sumUpnlPctRough)}
            cls={signClass(sumUpnlPctRough)}
          />
          <div className="my-2 border-t border-zinc-800" />
          <RowKV
            k="Açık Pozisyon (exposure)"
            v={`${fmtUsd(data.open_exposure_usd)} $`}
            cls={data.open_exposure_usd > 0 ? "text-amber-300" : undefined}
          />
          <RowKV
            k="Ort. Allocation"
            v={`${fmtUsd(avgAllocation)} $`}
          />
          <RowKV
            k="Toplam Açık"
            v={`${data.open_count} işlem (${data.long_count} long / ${data.short_count} short)`}
          />
          <RowKV k="Nakit Pozisyon" v={fmtPct(data.cash_position_pct)} />
        </div>
      </div>

      {/* Summary strip — mirrors KAPANIŞ/KARLI/BAŞARI/… with open-trade semantics */}
      <div className="col-span-3 grid grid-cols-7 gap-2">
        <StatCell label="Açık" value={`${data.open_count}`} />
        <StatCell
          label="Long / Short"
          value={`${data.long_count}/${data.short_count}`}
        />
        <StatCell
          label="Kazananda"
          value={`${rows.filter((r) => (r.u_pnl_pct ?? 0) > 0).length}/${data.open_count}`}
          cls="text-amber-400"
        />
        <StatCell
          label="Toplam Bağlanan"
          value={`${fmtUsd(data.open_exposure_usd)} $`}
        />
        <StatCell
          label="Toplam u-PnL"
          value={`${fmtUsd(data.sum_u_pnl_usd)} $`}
          cls={signClass(data.sum_u_pnl_usd)}
        />
        <StatCell
          label="Ort. u-PnL %"
          value={fmtPct(data.avg_u_pnl_pct)}
          cls={signClass(data.avg_u_pnl_pct)}
        />
        <StatCell
          label="İşlem Başına Ort. Bağlanan"
          value={`${fmtUsd(avgAllocation)} $`}
        />
      </div>

      {/* PERFORMANS + RİSK + RİSK MODU — same 3-col layout */}
      <div className="rounded border border-zinc-800 bg-zinc-950/40">
        <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-amber-300">
          ■ Performans Göstergeleri (Anlık)
        </div>
        <div className="space-y-2 p-3 text-xs">
          <RowKV
            k="Kazanan Oran"
            v={fmtPct(
              data.open_count > 0
                ? (rows.filter((r) => (r.u_pnl_pct ?? 0) > 0).length /
                    data.open_count) *
                    100
                : null,
              1,
            )}
          />
          <RowKV k="Avg Allocation" v={`${fmtUsd(avgAllocation)} $`} />
          <RowKV k="Sermaye Kullanımı" v={fmtPct(capitalUse, 2)} />
          <RowKV
            k="Ort. u-PnL"
            v={fmtPct(data.avg_u_pnl_pct)}
            cls={signClass(data.avg_u_pnl_pct)}
          />
          <RowKV
            k="Bileşik Getiri (u)"
            v={fmtPct(sumUpnlPctRough)}
            cls={signClass(sumUpnlPctRough)}
          />
          <RowKV
            k="Ort. Açık Süre"
            v={
              rows.length > 0
                ? formatAge(
                    new Date(
                      rows.reduce(
                        (acc, r) => acc + new Date(r.opened_at).getTime(),
                        0,
                      ) / rows.length,
                    ).toISOString(),
                  )
                : "—"
            }
          />
        </div>
      </div>

      <div className="rounded border border-zinc-800 bg-zinc-950/40">
        <div className="border-b border-zinc-800 px-3 py-2 text-[10px] uppercase tracking-wider text-amber-300">
          ■ Risk Metrikleri (Anlık)
        </div>
        <div className="space-y-2 p-3 text-xs">
          <div className="flex items-center justify-between">
            <span className="text-zinc-500">Risk Modu</span>
            <RiskModeBadge mode={riskMode} />
          </div>
          <RowKV
            k="Volatilite Seviyesi"
            v={
              Math.abs(data.avg_u_pnl_pct ?? 0) > 2
                ? "YÜKSEK"
                : Math.abs(data.avg_u_pnl_pct ?? 0) > 0.5
                  ? "ORTA"
                  : "DÜŞÜK"
            }
          />
          <RowKV k="Portföy Dağılımı" v={spread} />
          <RowKV k="Sembol Sayısı" v={`${data.distinct_symbols}`} />
          <RowKV k="Korelasyon Risk" v={corrRisk} />
          <RowKV k="Nakit Pozisyon" v={fmtPct(data.cash_position_pct)} />
        </div>
      </div>

      <div className="rounded border border-zinc-800 bg-zinc-950/40 p-3">
        <div className="mb-2 text-[10px] uppercase tracking-wider text-amber-300">
          ■ Risk Modu
        </div>
        <div className="flex flex-col items-center gap-3 py-4">
          <RiskModeBadge mode={riskMode} />
          <div className="flex gap-2">
            <span
              className={`rounded px-2 py-0.5 text-[10px] ${
                riskMode === "risk_off"
                  ? "bg-rose-500/30 text-rose-200"
                  : "bg-zinc-800 text-zinc-500"
              }`}
            >
              RISK-OFF
            </span>
            <span
              className={`rounded px-2 py-0.5 text-[10px] ${
                riskMode === "neutral"
                  ? "bg-amber-500/30 text-amber-200"
                  : "bg-zinc-800 text-zinc-500"
              }`}
            >
              NÖTR
            </span>
            <span
              className={`rounded px-2 py-0.5 text-[10px] ${
                riskMode === "risk_on"
                  ? "bg-emerald-500/30 text-emerald-200"
                  : "bg-zinc-800 text-zinc-500"
              }`}
            >
              RISK-ON
            </span>
          </div>
          <div className="mt-3 text-xs uppercase tracking-widest text-zinc-500">
            Nakit Pozisyon
          </div>
          <div
            className={`text-2xl font-bold ${
              data.cash_position_pct >= 90
                ? "text-emerald-300"
                : data.cash_position_pct >= 40
                  ? "text-amber-300"
                  : "text-rose-300"
            }`}
          >
            {fmtPct(data.cash_position_pct, 2)}
          </div>
        </div>
      </div>
      {selected && (
        <LiveTradeDrawer trade={selected} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}

// Drawer for an open position — shows entry / SL / TP ladder with
// signed deltas against live mark. Mirrors the Setups page drawer so
// the user's eye reads the same layout across views.
function LiveTradeDrawer({
  trade,
  onClose,
}: {
  trade: LiveTrade;
  onClose: () => void;
}) {
  const isLong = trade.direction === "long" || trade.direction === "buy";
  const entry = trade.entry_price;
  const mark = trade.mark_price ?? entry;
  const ladder = Array.isArray(trade.tp_ladder) ? trade.tp_ladder : [];

  const pctFromEntry = (other: number | null | undefined, favourable: boolean) => {
    if (other == null || entry <= 0) return null;
    const raw = ((other - entry) / entry) * 100;
    const signed = isLong ? raw : -raw;
    return favourable ? signed : signed;
  };

  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="h-full w-[520px] overflow-y-auto border-l border-zinc-800 bg-zinc-950 p-4 text-sm"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
          <div>
            <div className="text-lg font-bold text-amber-300">
              {trade.symbol}{" "}
              <span className="text-xs text-zinc-500">· {trade.timeframe}</span>
              {trade.profile && (
                <span className="ml-2 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] uppercase text-zinc-300">
                  {trade.profile}
                </span>
              )}
            </div>
            <div className="text-[10px] uppercase tracking-widest text-zinc-500">
              {trade.setup_id ?? "—"}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-300 hover:bg-zinc-700"
          >
            ×
          </button>
        </div>

        {/* Price panel — Giriş / Mark / SL / Target */}
        <section className="mt-4 rounded border border-zinc-800 bg-gradient-to-br from-zinc-900 to-zinc-950 p-3">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-xs uppercase tracking-widest text-zinc-500">
              Fiyat Seviyeleri
            </h3>
            <div className="flex gap-2">
              <span
                className={`rounded px-2 py-0.5 text-[10px] uppercase ${
                  isLong
                    ? "bg-emerald-500/20 text-emerald-200"
                    : "bg-rose-500/20 text-rose-200"
                }`}
              >
                {isLong ? "LONG" : "SHORT"}
              </span>
              <span className="rounded bg-sky-500/20 px-2 py-0.5 text-[10px] uppercase text-sky-200">
                {trade.mode}
              </span>
              {trade.leverage > 1 && (
                <span className="rounded bg-amber-500/20 px-2 py-0.5 text-[10px] uppercase text-amber-300">
                  {trade.leverage}x
                </span>
              )}
            </div>
          </div>
          <div className="grid grid-cols-4 gap-2 text-center">
            <DrawerCell
              label="Açılış Fiyatı"
              value={entry.toFixed(6)}
              sub={
                trade.setup_entry_price != null &&
                Math.abs(trade.setup_entry_price - entry) > 1e-9
                  ? `Hedef: ${trade.setup_entry_price.toFixed(6)}`
                  : undefined
              }
              accent="text-zinc-100"
            />
            <DrawerCell
              label="Anlık"
              value={mark.toFixed(6)}
              sub={fmtPct(trade.u_pnl_pct)}
              accent="text-amber-300"
              subCls={signClass(trade.u_pnl_pct)}
            />
            <DrawerCell
              label="Stop (SL)"
              value={trade.sl != null ? trade.sl.toFixed(6) : "—"}
              sub={fmtPct(pctFromEntry(trade.sl, false))}
              accent="text-rose-300"
              subCls="text-rose-400"
            />
            <DrawerCell
              label="Kâr Al (TP)"
              value={trade.tp != null ? trade.tp.toFixed(6) : "—"}
              sub={fmtPct(pctFromEntry(trade.tp, true))}
              accent="text-emerald-300"
              subCls="text-emerald-400"
            />
          </div>
        </section>

        {/* TP Ladder */}
        {ladder.length > 0 && (
          <section className="mt-4">
            <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
              TP Merdiveni
            </h3>
            <table className="w-full border-collapse text-xs">
              <thead className="text-left text-zinc-500">
                <tr>
                  <th className="px-2 py-1">#</th>
                  <th className="px-2 py-1 text-right">Fiyat</th>
                  <th className="px-2 py-1 text-right">Δ %</th>
                  <th className="px-2 py-1 text-right">Qty%</th>
                </tr>
              </thead>
              <tbody>
                {ladder.map((t, i) => {
                  const p = pctFromEntry(t.price ?? null, true);
                  const q = t.size_pct ?? t.qty;
                  return (
                    <tr key={i} className="border-t border-zinc-900">
                      <td className="px-2 py-1">TP{i + 1}</td>
                      <td className="px-2 py-1 text-right font-mono text-emerald-300">
                        {t.price != null ? t.price.toFixed(6) : "—"}
                      </td>
                      <td className="px-2 py-1 text-right font-mono text-emerald-300">
                        {fmtPct(p)}
                      </td>
                      <td className="px-2 py-1 text-right font-mono">
                        {q != null ? `${(Number(q) * 100).toFixed(0)}%` : "—"}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>
        )}

        {/* Pozisyon bilgileri */}
        <section className="mt-4 rounded border border-zinc-800 bg-zinc-900/40 p-3">
          <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
            Pozisyon
          </h3>
          <dl className="grid grid-cols-2 gap-y-1 text-xs">
            <KV k="Miktar (qty)" v={trade.qty.toFixed(4)} />
            <KV k="Notional" v={`${fmtUsd(trade.notional_usd)} $`} />
            <KV k="Kaldıraç" v={`${trade.leverage}x`} />
            <KV
              k="Açık u-PnL"
              v={
                <span className={signClass(trade.u_pnl_usd)}>
                  {trade.u_pnl_usd != null
                    ? `${trade.u_pnl_usd >= 0 ? "+" : ""}${fmtUsd(trade.u_pnl_usd)} $`
                    : "—"}
                </span>
              }
            />
            <KV k="Açık Süre" v={formatAge(trade.opened_at)} />
            <KV
              k="Açılış"
              v={new Date(trade.opened_at).toLocaleString("tr-TR")}
            />
          </dl>
        </section>
      </div>
    </div>
  );
}

function DrawerCell({
  label,
  value,
  sub,
  accent,
  subCls,
}: {
  label: string;
  value: string;
  sub?: string;
  accent?: string;
  subCls?: string;
}) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-950 p-2">
      <div className="text-[10px] uppercase tracking-widest text-zinc-500">
        {label}
      </div>
      <div
        className={`mt-1 font-mono text-base font-semibold ${accent ?? "text-zinc-100"}`}
      >
        {value}
      </div>
      {sub && (
        <div className={`mt-0.5 font-mono text-[11px] ${subCls ?? "text-zinc-400"}`}>
          {sub}
        </div>
      )}
    </div>
  );
}

function KV({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <>
      <dt className="uppercase tracking-widest text-zinc-500">{k}</dt>
      <dd className="font-mono text-zinc-200">{v}</dd>
    </>
  );
}

function formatAge(opened: string): string {
  const ms = Date.now() - new Date(opened).getTime();
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs} sn`;
  if (secs < 3600) return `${Math.floor(secs / 60)} dk`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h < 24) return `${h} sa ${m} dk`;
  const d = Math.floor(h / 24);
  const hh = h % 24;
  return `${d} g ${hh} sa`;
}
