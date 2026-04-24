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

type Period = "daily" | "weekly" | "monthly" | "yearly";
type Market = "coin";
type Mode = "live" | "dry" | "backtest";

interface RadarTrade {
  symbol: string;
  direction: string;
  date: string;
  notional_usd: number;
  pnl_usd: number;
  return_pct: number;
  status: string;
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

  const q = useQuery<RadarResponse>({
    queryKey: ["radar", market, period, mode],
    queryFn: () =>
      apiFetch(`/v2/radar/${market}/${period}?mode=${mode}&history=true`),
    refetchInterval: 60_000,
  });

  const latest = q.data?.latest ?? null;
  const trades = latest?.trades ?? [];

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
      </div>

      {q.isLoading && <div className="text-zinc-500">yükleniyor…</div>}
      {q.isError && (
        <div className="text-rose-400">
          hata: {(q.error as Error)?.message ?? "rapor çekilemedi"}
        </div>
      )}

      {latest && (
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
