// Analiz page — single-shot multi-symbol breakdown.
//
// User: "GUI de analiz isimli bir aç ve analiz sonuçlarını exchange,
// market ve sembol kırılımda orada göster. onchain, diptepe v.b."
//
// Renders a flat matrix grouped by exchange → segment → symbol with:
//   - Per-timeframe IQ wave / state / branch
//   - Per-timeframe Major Dip composite score + verdict pill
//   - Per-timeframe armed setup count
//   - Open positions + aggregate notional (across TFs)
//   - Latest onchain snapshot age + kind (Nansen)
//
// Refresh on a 30s cadence; collapse/expand per exchange group.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

type TfSnapshot = {
  timeframe: string;
  iq_state?: string | null;
  iq_wave?: string | null;
  primary_branch?: string | null;
  major_dip_score?: number | null;
  major_dip_verdict?: string | null;
  iq_setups_armed: number;
  last_advanced_at?: string | null;
};

type AnalizRow = {
  exchange: string;
  segment: string;
  symbol: string;
  timeframes: TfSnapshot[];
  open_positions: number;
  aggregate_notional_usd: number;
  onchain_snapshot_age_s?: number | null;
  onchain_kind?: string | null;
};

type AnalizResp = {
  generated_at: string;
  rows: AnalizRow[];
};

// Verdict pill — same color scheme as the IQ Chart sidebar so the
// reader builds one mental model for both views.
function VerdictPill({ verdict, score }: { verdict?: string | null; score?: number | null }) {
  if (!verdict || score == null) {
    return <span className="rounded bg-zinc-800/60 px-1 text-[9px] text-zinc-500">—</span>;
  }
  const cls =
    verdict === "very_high"
      ? "bg-emerald-500/30 text-emerald-200"
      : verdict === "high"
      ? "bg-emerald-500/20 text-emerald-300"
      : verdict === "developing"
      ? "bg-amber-500/20 text-amber-300"
      : "bg-zinc-700/40 text-zinc-400";
  return (
    <span className={`rounded px-1 font-mono text-[9px] ${cls}`}>
      {(score * 100).toFixed(0)}%
    </span>
  );
}

// Compact wave + branch indicator. Highlights "C" + flat_expanded
// etc. so the user spots non-zigzag corrections at a glance.
function WaveCell({ tf }: { tf: TfSnapshot }) {
  if (!tf.iq_state) {
    return <span className="text-zinc-600">—</span>;
  }
  const stateColor: Record<string, string> = {
    candidate: "text-zinc-400",
    tracking: "text-emerald-300",
    completed: "text-cyan-300",
    invalidated: "text-zinc-500",
  };
  const bc = (tf.primary_branch || "").replace(/^flat_/, "f.");
  return (
    <span className={`font-mono text-[10px] ${stateColor[tf.iq_state] ?? "text-zinc-300"}`}>
      {tf.iq_wave ?? "?"}
      {bc && <span className="ml-0.5 text-[8px] text-zinc-500">·{bc}</span>}
    </span>
  );
}

// Format a notional as $X.XK / $X.XM.
function fmtNotional(usd: number): string {
  if (!Number.isFinite(usd) || usd === 0) return "—";
  if (usd >= 1_000_000) return `$${(usd / 1_000_000).toFixed(2)}M`;
  if (usd >= 1_000) return `$${(usd / 1_000).toFixed(1)}K`;
  return `$${usd.toFixed(0)}`;
}

export default function Analiz() {
  const { data, isLoading } = useQuery<AnalizResp>({
    queryKey: ["analiz"],
    queryFn: () => apiFetch("/v2/analiz"),
    refetchInterval: 30_000,
  });

  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const grouped = useMemo(() => {
    const m = new Map<string, AnalizRow[]>();
    for (const r of data?.rows ?? []) {
      const k = `${r.exchange} · ${r.segment}`;
      if (!m.has(k)) m.set(k, []);
      m.get(k)!.push(r);
    }
    return Array.from(m.entries()).sort((a, b) => a[0].localeCompare(b[0]));
  }, [data]);

  const allTfs = useMemo(() => {
    const set = new Set<string>();
    for (const r of data?.rows ?? []) {
      for (const tf of r.timeframes) set.add(tf.timeframe);
    }
    // Canonical order
    const order = ["1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "12h", "1d", "3d", "1w", "1M"];
    return Array.from(set).sort(
      (a, b) => (order.indexOf(a) === -1 ? 99 : order.indexOf(a)) - (order.indexOf(b) === -1 ? 99 : order.indexOf(b)),
    );
  }, [data]);

  return (
    <div className="p-4 text-sm">
      <div className="mb-4 flex items-baseline justify-between border-b border-zinc-800 pb-2">
        <div>
          <h1 className="text-lg font-bold text-amber-300">QTSS Analiz</h1>
          <p className="text-[11px] text-zinc-500">
            Exchange · Market · Sembol kırılımında IQ dalgaları, Major Dip
            skorları, açık pozisyonlar ve onchain durum.
          </p>
        </div>
        <div className="text-[10px] text-zinc-500">
          {data && new Date(data.generated_at).toLocaleString("tr-TR")}
        </div>
      </div>

      {isLoading && <div className="text-zinc-500">Yükleniyor…</div>}

      {grouped.map(([groupKey, rows]) => {
        const open = expanded[groupKey] ?? true;
        return (
          <section key={groupKey} className="mb-4">
            <button
              type="button"
              className="mb-2 flex w-full items-center justify-between rounded bg-zinc-900/60 px-3 py-1 text-left"
              onClick={() => setExpanded((e) => ({ ...e, [groupKey]: !open }))}
            >
              <span className="font-mono text-xs uppercase tracking-wider text-zinc-300">
                {groupKey}
              </span>
              <span className="text-[10px] text-zinc-500">{open ? "−" : "+"} {rows.length}</span>
            </button>
            {open && (
              <div className="overflow-x-auto rounded border border-zinc-800">
                <table className="w-full text-[11px]">
                  <thead className="bg-zinc-900/40">
                    <tr className="text-left text-[9px] uppercase tracking-wider text-zinc-500">
                      <th className="px-2 py-1">Sembol</th>
                      {allTfs.map((tf) => (
                        <th key={tf} className="px-2 py-1 text-center">
                          {tf}
                        </th>
                      ))}
                      <th className="px-2 py-1 text-right">Açık Poz</th>
                      <th className="px-2 py-1 text-right">Notional</th>
                      <th className="px-2 py-1 text-right">Onchain</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row) => {
                      const ageMin = row.onchain_snapshot_age_s
                        ? Math.round(row.onchain_snapshot_age_s / 60)
                        : null;
                      return (
                        <tr key={row.symbol} className="border-t border-zinc-800/60 hover:bg-zinc-900/40">
                          <td className="px-2 py-1 font-mono font-semibold text-zinc-200">
                            {row.symbol}
                          </td>
                          {allTfs.map((tfKey) => {
                            const tf = row.timeframes.find((x) => x.timeframe === tfKey);
                            if (!tf) {
                              return (
                                <td key={tfKey} className="px-2 py-1 text-center text-zinc-700">
                                  —
                                </td>
                              );
                            }
                            return (
                              <td key={tfKey} className="px-2 py-1">
                                <div className="flex items-center justify-between gap-1">
                                  <WaveCell tf={tf} />
                                  <VerdictPill
                                    verdict={tf.major_dip_verdict}
                                    score={tf.major_dip_score}
                                  />
                                </div>
                                {tf.iq_setups_armed > 0 && (
                                  <div className="mt-0.5 text-center text-[8px] text-amber-400">
                                    {tf.iq_setups_armed} armed
                                  </div>
                                )}
                              </td>
                            );
                          })}
                          <td className="px-2 py-1 text-right font-mono">
                            {row.open_positions || <span className="text-zinc-700">—</span>}
                          </td>
                          <td className="px-2 py-1 text-right font-mono text-zinc-300">
                            {fmtNotional(row.aggregate_notional_usd)}
                          </td>
                          <td className="px-2 py-1 text-right font-mono text-[10px]">
                            {row.onchain_kind ? (
                              <span className="text-cyan-400">
                                {row.onchain_kind}
                                {ageMin != null && (
                                  <span className="ml-1 text-zinc-500">{ageMin}m</span>
                                )}
                              </span>
                            ) : (
                              <span className="text-zinc-700">—</span>
                            )}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        );
      })}

      {!isLoading && (data?.rows ?? []).length === 0 && (
        <div className="rounded border border-zinc-800 bg-zinc-900/40 p-3 text-zinc-500">
          Aktif sembol yok. Engine Symbols sayfasından sembol etkinleştir.
        </div>
      )}
    </div>
  );
}
