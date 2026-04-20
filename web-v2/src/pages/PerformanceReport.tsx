// PerformanceReport — Faz 15.R
//
// QTSS performans raporu: günlük / haftalık / aylık / yıllık × borsa sınıfı
// (Crypto / NASDAQ / BIST). Sanal cüzdan mantığıyla
// `qtss_v2_detection_outcomes` verisinden üretilir; Faz 15 paper ledger
// devreye girdiğinde backend aynı şema ile gerçek veriye geçer.
//
// Tasarım: üst sekmeler (period + exchange class) → "Kapanan Analizler"
// tablosu → 3 büyük sayaç (kapanış / kârlı / başarı %) → orta KPI şeridi
// (toplam başlanan / toplam kazanç / ortalama poz getirisi) → sermaye
// takibi kartı (bileşik getiri + allocation + win rate + holding period).
// Slogan ve sosyal handle eklenmez.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { apiFetch } from "../lib/api";

type Period = "daily" | "weekly" | "monthly" | "yearly";
type ExClass = "all" | "crypto" | "nasdaq" | "bist";

interface ClosedTrade {
  symbol: string;
  exchange: string;
  tip: string;
  resolved_at: string;
  allocation: number;
  pnl: number;
  pnl_pct: number;
  outcome: "win" | "loss";
  holding_hours: number;
}

interface PerformanceReportDto {
  generated_at: string;
  exchange_class: string;
  period: string;
  period_start: string;
  period_end: string;
  currency: string;
  total_allocated: number;
  total_pnl: number;
  avg_pnl_pct: number;
  closed_count: number;
  win_count: number;
  loss_count: number;
  win_rate: number;
  starting_equity: number;
  current_equity: number;
  compound_return: number;
  avg_allocation: number;
  avg_holding_hours: number;
  trades: ClosedTrade[];
}

const PERIODS: { key: Period; label: string }[] = [
  { key: "daily",   label: "Günlük" },
  { key: "weekly",  label: "Haftalık" },
  { key: "monthly", label: "Aylık" },
  { key: "yearly",  label: "Yıllık" },
];

const CLASSES: { key: ExClass; label: string }[] = [
  { key: "all",    label: "Tümü" },
  { key: "crypto", label: "Crypto" },
  { key: "nasdaq", label: "NASDAQ" },
  { key: "bist",   label: "BIST" },
];

// Para birimi-duyarlı formatlama. Binance raporunda $, BIST'te TL.
function fmtMoney(n: number, currency: string, digits = 0) {
  const sign = n < 0 ? "-" : n > 0 ? "+" : "";
  const abs  = Math.abs(n);
  const compact =
    abs >= 1_000_000 ? `${(abs / 1_000_000).toFixed(2)}M` :
    abs >= 1_000     ? `${(abs / 1_000).toFixed(2)}K`     :
                       abs.toFixed(digits);
  const sym = currency === "TL" ? "TL" : currency === "USD" ? "$" : currency;
  return currency === "TL" ? `${sign}${compact} ${sym}` : `${sign}${compact} ${sym}`;
}

function fmtFull(n: number, currency: string) {
  const rounded = Math.round(n).toLocaleString("tr-TR");
  return currency === "TL" ? `${rounded} TL` : `${rounded} ${currency}`;
}

function fmtPct(x: number, digits = 2) {
  const sign = x > 0 ? "+" : "";
  return `${sign}${(x * 100).toFixed(digits)}%`;
}

function fmtDate(iso: string) {
  const d = new Date(iso);
  const months = ["OCA","ŞUB","MAR","NİS","MAY","HAZ","TEM","AĞU","EYL","EKİ","KAS","ARA"];
  return `${String(d.getDate()).padStart(2, "0")} ${months[d.getMonth()]}`;
}

function periodLabel(p: Period) {
  return PERIODS.find((x) => x.key === p)?.label ?? p;
}

export function PerformanceReport() {
  const [period, setPeriod]   = useState<Period>("monthly");
  const [exClass, setExClass] = useState<ExClass>("crypto");

  const { data, isLoading, error } = useQuery<PerformanceReportDto>({
    queryKey: ["v2-reports-performance", period, exClass],
    queryFn: async () => {
      const qs = new URLSearchParams({ period, exchange_class: exClass });
      return apiFetch<PerformanceReportDto>(`/v2/reports/performance?${qs.toString()}`);
    },
    refetchInterval: 60_000,
  });

  return (
    <div className="min-h-screen bg-zinc-950 p-6 text-zinc-100">
      <div className="mx-auto max-w-4xl">
        {/* Başlık */}
        <div className="mb-8 flex items-end justify-between border-b border-zinc-800 pb-4">
          <div>
            <div className="text-3xl font-bold tracking-[0.3em] text-emerald-400">
              QTSS
            </div>
            <div className="text-xs tracking-[0.4em] text-zinc-500">
              RAPOR · {periodLabel(period).toUpperCase()}
            </div>
          </div>
          <div className="text-right text-xs text-zinc-500">
            <div className="tracking-widest">
              {data ? `${fmtDate(data.period_start)} → ${fmtDate(data.period_end)}` : "—"}
            </div>
            <div className="mt-1 text-zinc-600">Ara durum</div>
          </div>
        </div>

        {/* Sekmeler — period + exchange class */}
        <div className="mb-6 flex flex-wrap gap-3">
          <div className="flex gap-1 rounded border border-zinc-800 bg-zinc-900 p-1">
            {PERIODS.map((p) => (
              <button
                key={p.key}
                onClick={() => setPeriod(p.key)}
                className={
                  "px-3 py-1.5 text-xs tracking-wider transition " +
                  (period === p.key
                    ? "bg-emerald-500/20 text-emerald-300"
                    : "text-zinc-400 hover:text-zinc-200")
                }
              >
                {p.label.toUpperCase()}
              </button>
            ))}
          </div>
          <div className="flex gap-1 rounded border border-zinc-800 bg-zinc-900 p-1">
            {CLASSES.map((c) => (
              <button
                key={c.key}
                onClick={() => setExClass(c.key)}
                className={
                  "px-3 py-1.5 text-xs tracking-wider transition " +
                  (exClass === c.key
                    ? "bg-emerald-500/20 text-emerald-300"
                    : "text-zinc-400 hover:text-zinc-200")
                }
              >
                {c.label.toUpperCase()}
              </button>
            ))}
          </div>
        </div>

        {isLoading && <div className="text-zinc-500">Yükleniyor…</div>}
        {error && <div className="text-red-400">Rapor yüklenemedi.</div>}
        {data && <ReportBody data={data} />}
      </div>
    </div>
  );
}

function ReportBody({ data }: { data: PerformanceReportDto }) {
  const cur = data.currency;
  return (
    <>
      {/* Kapanan Analizler tablosu */}
      <div className="mb-8">
        <div className="mb-3 flex items-center gap-2">
          <span className="h-2 w-2 bg-emerald-400" />
          <h2 className="text-xs font-bold tracking-[0.3em] text-emerald-400">
            KAPANAN ANALİZLER
          </h2>
        </div>

        {data.trades.length === 0 ? (
          <div className="py-6 text-center text-sm text-zinc-600">
            Bu dönemde kapanmış işlem yok.
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="text-[10px] tracking-widest text-zinc-600">
                <th className="py-2 text-left">VARLIK</th>
                <th className="py-2 text-left">TİP · TARİH</th>
                <th className="py-2 text-right">BAŞLANAN → KAZANÇ</th>
                <th className="py-2 text-right">GETİRİ</th>
              </tr>
            </thead>
            <tbody>
              {data.trades.map((t, i) => {
                const win = t.pnl >= 0;
                return (
                  <tr key={i} className="border-t border-zinc-900">
                    <td className="py-2">
                      <span className={"mr-2 inline-block h-2 w-2 rounded-full " + (win ? "bg-emerald-400" : "bg-red-400")} />
                      <span className="font-bold tracking-wider">{t.symbol.toUpperCase()}</span>
                    </td>
                    <td className="py-2 text-zinc-500">
                      {t.tip} · {fmtDate(t.resolved_at)}
                    </td>
                    <td className="py-2 text-right text-zinc-400 tabular-nums">
                      <span className="text-zinc-500">{fmtMoney(t.allocation, cur)}</span>
                      <span className="mx-1 text-zinc-700">→</span>
                      <span className={win ? "text-emerald-400" : "text-red-400"}>
                        {fmtMoney(t.pnl, cur)}
                      </span>
                    </td>
                    <td className={"py-2 text-right tabular-nums font-semibold " + (win ? "text-emerald-400" : "text-red-400")}>
                      {fmtPct(t.pnl_pct)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* 3 büyük sayaç */}
      <div className="mb-6 grid grid-cols-3 gap-4 border-y border-zinc-900 py-8">
        <BigStat value={String(data.closed_count)} label="KAPANIŞ" tone="emerald" />
        <BigStat
          value={`${data.win_count}/${data.closed_count}`}
          label="KÂRLI"
          tone="emerald"
        />
        <BigStat value={fmtPct(data.win_rate, 0)} label="BAŞARI" tone="yellow" />
      </div>

      {/* Orta KPI şeridi */}
      <div className="mb-8 grid grid-cols-3 gap-4">
        <MidStat value={fmtMoney(data.total_allocated, cur)} label="TOPLAM BAŞLANAN" />
        <MidStat
          value={fmtMoney(data.total_pnl, cur)}
          label="TOPLAM KAZANÇ"
          tone={data.total_pnl >= 0 ? "emerald" : "red"}
        />
        <MidStat
          value={fmtPct(data.avg_pnl_pct)}
          label="ORT. POZ. GETİRİSİ"
          tone={data.avg_pnl_pct >= 0 ? "emerald" : "red"}
        />
      </div>

      {/* Sermaye takibi */}
      <div className="mb-6 border-l-2 border-emerald-400 bg-zinc-900/50 p-5">
        <h3 className="mb-4 text-xs font-bold tracking-[0.3em] text-emerald-400">
          SERMAYE TAKİBİ — BİLEŞİK GETİRİ
        </h3>
        <Row label={`Başlangıç (${fmtDate(data.period_start)})`} value={fmtFull(data.starting_equity, cur)} />
        <Row
          label="Toplam kapanış"
          value={`${data.closed_count} işlem (${data.win_count} kârlı / ${data.loss_count} zararlı)`}
        />
        <Row
          label="Toplam realize kazanç"
          value={fmtMoney(data.total_pnl, cur)}
          tone={data.total_pnl >= 0 ? "emerald" : "red"}
        />
        <Row label="Güncel sermaye" value={fmtFull(data.current_equity, cur)} tone="emerald" />
        <Row
          label="Bileşik getiri"
          value={fmtPct(data.compound_return)}
          tone={data.compound_return >= 0 ? "emerald" : "red"}
          bold
        />
        <div className="my-3 border-t border-zinc-800" />
        <Row label="Avg. Allocation (işlem başına)" value={fmtMoney(data.avg_allocation, cur)} muted />
        <Row label="Win Rate" value={fmtPct(data.win_rate, 1)} muted />
        <Row
          label="Avg. Holding Period"
          value={
            data.avg_holding_hours >= 24
              ? `${(data.avg_holding_hours / 24).toFixed(1)} gün`
              : `${data.avg_holding_hours.toFixed(1)} saat`
          }
          muted
        />
      </div>
    </>
  );
}

function BigStat({
  value,
  label,
  tone,
}: {
  value: string;
  label: string;
  tone: "emerald" | "yellow" | "red";
}) {
  const color =
    tone === "emerald"
      ? "text-emerald-400"
      : tone === "yellow"
      ? "text-yellow-400"
      : "text-red-400";
  return (
    <div className="text-center">
      <div className={"text-5xl font-bold tabular-nums " + color}>{value}</div>
      <div className="mt-2 text-[10px] tracking-[0.3em] text-zinc-500">{label}</div>
    </div>
  );
}

function MidStat({
  value,
  label,
  tone = "emerald",
}: {
  value: string;
  label: string;
  tone?: "emerald" | "red";
}) {
  const color = tone === "emerald" ? "text-emerald-400" : "text-red-400";
  return (
    <div className="text-center">
      <div className={"text-2xl font-bold tabular-nums " + color}>{value}</div>
      <div className="mt-1 text-[10px] tracking-[0.3em] text-zinc-500">{label}</div>
    </div>
  );
}

function Row({
  label,
  value,
  tone,
  bold,
  muted,
}: {
  label: string;
  value: string;
  tone?: "emerald" | "red";
  bold?: boolean;
  muted?: boolean;
}) {
  const color =
    tone === "emerald" ? "text-emerald-400" :
    tone === "red"     ? "text-red-400"     :
    muted              ? "text-zinc-400"    :
                         "text-zinc-200";
  return (
    <div className="flex items-center justify-between py-1.5 text-sm">
      <span className={muted ? "text-zinc-500" : "text-zinc-400"}>{label}</span>
      <span className={`tabular-nums ${color} ${bold ? "font-bold" : ""}`}>{value}</span>
    </div>
  );
}
