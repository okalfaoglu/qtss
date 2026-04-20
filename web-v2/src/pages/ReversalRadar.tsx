// Dip/Tepe Radarı — Faz 13
//
// Reactive (L0/L1) ve Major (L2/L3) dönüş noktalarını tek listede
// tarar. Veri kaynağı: `/v2/reversal-radar` (pivot_reversal aile).
// Filtreler: tier / event / direction / level / symbol / timeframe / mode.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

interface TargetsA {
  entry: number;
  sl: number;
  tp1: number;
  tp2: number;
  tp1_r: number;
  tp2_r: number;
  risk_dist: number;
}
interface TargetsB {
  impulse_lo: number;
  impulse_hi: number;
  fib: Record<string, number>;
}
interface TargetsPack {
  a?: TargetsA;
  b?: TargetsB;
}

interface RadarEntry {
  id: string;
  detected_at: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  subkind: string;
  state: string;
  mode: string;
  pivot_level: string;
  structural_score: number;
  invalidation_price: number | null;
  tier: string;
  event: string;
  direction: string;
  outcome: string | null;
  pnl_pct: number | null;
  close_reason: string | null;
  targets: TargetsPack | null;
}

interface RadarFeed {
  generated_at: string;
  entries: RadarEntry[];
}

const TIER_COLOR: Record<string, string> = {
  major:    "bg-cyan-500/20 text-cyan-200 border-cyan-400/40",
  reactive: "bg-zinc-600/30 text-zinc-200 border-zinc-500/40",
};

const EVENT_COLOR: Record<string, string> = {
  choch:   "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  bos:     "bg-sky-500/15 text-sky-300 border-sky-500/30",
  neutral: "bg-zinc-700/40 text-zinc-400 border-zinc-600/40",
};

const DIR_COLOR: Record<string, string> = {
  bull: "text-emerald-400",
  bear: "text-rose-400",
};

const OUTCOME_COLOR: Record<string, string> = {
  win:     "text-emerald-400",
  loss:    "text-rose-400",
  expired: "text-amber-400",
};

function fmtTime(iso: string): string {
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

// Fiyatları kompakt göster: binler ayırıcı + dinamik ondalık.
function fmtPrice(v: number | undefined | null): string {
  if (v == null || !Number.isFinite(v)) return "—";
  const abs = Math.abs(v);
  const digits = abs >= 1000 ? 1 : abs >= 10 ? 2 : 4;
  return v.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

export function ReversalRadar() {
  const [tier, setTier] = useState("");
  const [event, setEvent] = useState("");
  const [direction, setDirection] = useState("");
  const [level, setLevel] = useState("");
  const [symbol, setSymbol] = useState("");
  const [timeframe, setTimeframe] = useState("");
  const [mode, setMode] = useState("backtest");

  const { data, isLoading, isError, refetch } = useQuery<RadarFeed>({
    queryKey: ["reversal-radar", tier, event, direction, level, symbol, timeframe, mode],
    queryFn: () => {
      const p = new URLSearchParams();
      if (tier)      p.set("tier", tier);
      if (event)     p.set("event", event);
      if (direction) p.set("direction", direction);
      if (level)     p.set("level", level);
      if (symbol)    p.set("symbol", symbol);
      if (timeframe) p.set("timeframe", timeframe);
      if (mode)      p.set("mode", mode);
      p.set("limit", "500");
      return apiFetch<RadarFeed>(`/v2/reversal-radar?${p.toString()}`);
    },
    refetchOnWindowFocus: false,
  });

  const stats = useMemo(() => {
    const entries = data?.entries ?? [];
    const byTier: Record<string, number> = { major: 0, reactive: 0 };
    let wins = 0, losses = 0, expired = 0, active = 0;
    for (const e of entries) {
      byTier[e.tier] = (byTier[e.tier] ?? 0) + 1;
      if (e.outcome === "win") wins++;
      else if (e.outcome === "loss") losses++;
      else if (e.outcome === "expired") expired++;
      else active++;
    }
    return { byTier, wins, losses, expired, active };
  }, [data]);

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-zinc-100">Dip/Tepe Radarı</h1>
          <p className="text-sm text-zinc-400">
            Reactive (L0/L1 · tepki) + Major (L2/L3 · dönüş) pivot bazlı
            tarama. Subkind: <code>{"{tier}_{event}_{direction}_{level}"}</code>.
          </p>
        </div>
        <button
          type="button"
          onClick={() => refetch()}
          className="rounded border border-zinc-700 px-3 py-1 text-sm text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
        >
          Yenile
        </button>
      </header>

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <FilterSelect value={tier} onChange={setTier} label="Tier"
          options={[["","Tümü"],["major","Major (L2/L3)"],["reactive","Reactive (L0/L1)"]]} />
        <FilterSelect value={event} onChange={setEvent} label="Event"
          options={[["","Tümü"],["choch","CHoCH"],["bos","BOS"],["neutral","Neutral"]]} />
        <FilterSelect value={direction} onChange={setDirection} label="Yön"
          options={[["","Tümü"],["bull","Bull"],["bear","Bear"]]} />
        <FilterSelect value={level} onChange={setLevel} label="Seviye"
          options={[["","Tümü"],["L0","L0"],["L1","L1"],["L2","L2"],["L3","L3"]]} />
        <FilterInput value={symbol} onChange={setSymbol} placeholder="Sembol (BTCUSDT)" />
        <FilterInput value={timeframe} onChange={setTimeframe} placeholder="TF (1h)" />
        <FilterSelect value={mode} onChange={setMode} label="Mod"
          options={[["backtest","Backtest"],["live","Live"]]} />
      </div>

      {/* Top KPI strip */}
      <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-6">
        <Kpi title="Toplam" value={data?.entries.length ?? 0} />
        <Kpi title="Major" value={stats.byTier.major} color="text-cyan-300" />
        <Kpi title="Reactive" value={stats.byTier.reactive} color="text-zinc-300" />
        <Kpi title="Kazanç" value={stats.wins} color="text-emerald-400" />
        <Kpi title="Kayıp" value={stats.losses} color="text-rose-400" />
        <Kpi title="Beklemede" value={stats.active + stats.expired} color="text-amber-300" />
      </div>

      {isLoading && <div className="text-zinc-400">Yükleniyor…</div>}
      {isError && <div className="text-rose-400">Veri alınamadı.</div>}

      {data && (
        <div className="overflow-auto rounded-lg border border-zinc-800">
          <table className="min-w-full text-sm">
            <thead className="bg-zinc-900/60 text-zinc-400">
              <tr>
                <Th>Zaman</Th>
                <Th>Sembol</Th>
                <Th>TF</Th>
                <Th>Level</Th>
                <Th>Tier</Th>
                <Th>Event</Th>
                <Th>Yön</Th>
                <Th className="text-right">Skor</Th>
                <Th className="text-right">Entry</Th>
                <Th className="text-right">SL</Th>
                <Th className="text-right">TP1</Th>
                <Th className="text-right">TP2</Th>
                <Th>State</Th>
                <Th>Outcome</Th>
                <Th className="text-right">PnL %</Th>
              </tr>
            </thead>
            <tbody>
              {data.entries.map((e) => (
                <tr key={e.id} className="border-t border-zinc-800 hover:bg-zinc-900/40">
                  <td className="whitespace-nowrap px-3 py-1.5 text-zinc-400">{fmtTime(e.detected_at)}</td>
                  <td className="px-3 py-1.5 font-medium text-zinc-100">{e.symbol}</td>
                  <td className="px-3 py-1.5 text-zinc-300">{e.timeframe}</td>
                  <td className="px-3 py-1.5 text-zinc-300">{e.pivot_level}</td>
                  <td className="px-3 py-1.5">
                    <span className={`rounded border px-2 py-0.5 text-[11px] uppercase ${TIER_COLOR[e.tier] ?? ""}`}>
                      {e.tier}
                    </span>
                  </td>
                  <td className="px-3 py-1.5">
                    <span className={`rounded border px-2 py-0.5 text-[11px] uppercase ${EVENT_COLOR[e.event] ?? ""}`}>
                      {e.event}
                    </span>
                  </td>
                  <td className={`px-3 py-1.5 font-medium ${DIR_COLOR[e.direction] ?? "text-zinc-400"}`}>
                    {e.direction}
                  </td>
                  <td className="px-3 py-1.5 text-right text-zinc-200">{e.structural_score.toFixed(2)}</td>
                  <td className="px-3 py-1.5 text-right text-zinc-200 tabular-nums">{fmtPrice(e.targets?.a?.entry)}</td>
                  <td className="px-3 py-1.5 text-right text-rose-300 tabular-nums">{fmtPrice(e.targets?.a?.sl)}</td>
                  <td className="px-3 py-1.5 text-right text-emerald-300 tabular-nums">{fmtPrice(e.targets?.a?.tp1)}</td>
                  <td className="px-3 py-1.5 text-right text-emerald-400 tabular-nums">{fmtPrice(e.targets?.a?.tp2)}</td>
                  <td className="px-3 py-1.5 text-zinc-400">{e.state}</td>
                  <td className={`px-3 py-1.5 ${OUTCOME_COLOR[e.outcome ?? ""] ?? "text-zinc-500"}`}>
                    {e.outcome ?? "—"}
                  </td>
                  <td className={`px-3 py-1.5 text-right ${
                    (e.pnl_pct ?? 0) > 0 ? "text-emerald-400"
                      : (e.pnl_pct ?? 0) < 0 ? "text-rose-400"
                      : "text-zinc-500"}`}>
                    {e.pnl_pct != null ? e.pnl_pct.toFixed(2) : "—"}
                  </td>
                </tr>
              ))}
              {data.entries.length === 0 && (
                <tr>
                  <td colSpan={15} className="px-3 py-6 text-center text-zinc-500">
                    Kayıt bulunamadı. Filtreyi gevşet veya sweep'i çalıştır.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function Th({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return (
    <th className={`px-3 py-2 text-left text-[11px] font-semibold uppercase tracking-wide ${className}`}>
      {children}
    </th>
  );
}

function Kpi({ title, value, color = "text-zinc-100" }: { title: string; value: number; color?: string }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3">
      <div className="text-[11px] uppercase tracking-wide text-zinc-500">{title}</div>
      <div className={`mt-1 text-2xl font-semibold ${color}`}>{value}</div>
    </div>
  );
}

function FilterSelect({
  value, onChange, label, options,
}: {
  value: string;
  onChange: (v: string) => void;
  label: string;
  options: Array<[string, string]>;
}) {
  return (
    <label className="flex items-center gap-1 text-xs text-zinc-400">
      <span>{label}:</span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200"
      >
        {options.map(([v, l]) => (
          <option key={v} value={v}>{l}</option>
        ))}
      </select>
    </label>
  );
}

function FilterInput({
  value, onChange, placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  return (
    <input
      type="text"
      value={value}
      onChange={(e) => onChange(e.target.value.toUpperCase())}
      placeholder={placeholder}
      className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200 w-36"
    />
  );
}
