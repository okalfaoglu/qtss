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
  major_top_score?: number | null;
  major_top_verdict?: string | null;
  dip_components?: Record<string, number | null> | null;
  top_components?: Record<string, number | null> | null;
  setup_lifecycle: Record<string, number>;
  // FAZ 25.4.A — Wyckoff phase + last event per TF.
  wyckoff_phase?: string | null;
  wyckoff_last_event?: string | null;
  wyckoff_event_age_min?: number | null;
  wyckoff_alignment_score?: number | null;
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
  onchain_score?: number | null;
  onchain_direction?: string | null;
  onchain_confidence?: number | null;
};

type AnalizResp = {
  generated_at: string;
  rows: AnalizRow[];
};

// Verdict pill — same color scheme as the IQ Chart sidebar so the
// reader builds one mental model for both views. Polarity-aware:
// dip uses emerald (bullish reversal candidate); top uses rose
// (bearish reversal candidate).
function VerdictPill({
  verdict,
  score,
  polarity,
  prefix,
}: {
  verdict?: string | null;
  score?: number | null;
  polarity: "dip" | "top";
  prefix: string;
}) {
  if (!verdict || score == null) {
    return <span className="rounded bg-zinc-800/60 px-1 text-[9px] text-zinc-200">{prefix}—</span>;
  }
  const dipCls: Record<string, string> = {
    very_high: "bg-emerald-500/30 text-emerald-200",
    high: "bg-emerald-500/20 text-emerald-300",
    developing: "bg-amber-500/20 text-amber-300",
    low: "bg-zinc-700/40 text-zinc-100",
  };
  const topCls: Record<string, string> = {
    very_high: "bg-rose-500/30 text-rose-200",
    high: "bg-rose-500/20 text-rose-300",
    developing: "bg-amber-500/20 text-amber-300",
    low: "bg-zinc-700/40 text-zinc-100",
  };
  const cls = (polarity === "dip" ? dipCls : topCls)[verdict] ?? "bg-zinc-700/40 text-zinc-100";
  return (
    <span className={`rounded px-1 font-mono text-[9px] ${cls}`}>
      {prefix}
      {(score * 100).toFixed(0)}%
    </span>
  );
}

// 8-component breakdown sparklines. Renders a tiny inline grid of
// per-component scores so the reader sees WHICH channels lit up.
function ComponentBars({ comps }: { comps?: Record<string, number | null> | null }) {
  if (!comps) return null;
  const entries: Array<[string, string]> = [
    ["structural_completion", "STR"],
    ["fib_retrace_quality", "FIB"],
    ["volume_capitulation", "VOL"],
    ["wyckoff_alignment", "WYC"],
    ["cvd_divergence", "CVD"],
    ["indicator_alignment", "IND"],
    ["sentiment_extreme", "SEN"],
    ["multi_tf_confluence", "MTF"],
    ["funding_oi_signals", "F/O"],
  ];
  return (
    <div className="mt-1 grid grid-cols-9 gap-0.5">
      {entries.map(([key, lbl]) => {
        const v = (comps[key] ?? 0) as number;
        const w = Math.max(0, Math.min(1, v)) * 100;
        const tone =
          v > 0.7 ? "bg-emerald-500/70" : v > 0.3 ? "bg-amber-500/70" : v > 0 ? "bg-rose-500/40" : "bg-zinc-800";
        return (
          <div key={key} className="flex flex-col items-center" title={`${lbl} ${v.toFixed(2)}`}>
            <div className="relative h-3 w-full overflow-hidden rounded-sm bg-zinc-900">
              <div className={`${tone} absolute bottom-0 w-full`} style={{ height: `${w}%` }} />
            </div>
            <span className="font-mono text-[7px] text-zinc-200">{lbl}</span>
          </div>
        );
      })}
    </div>
  );
}

// Wyckoff strip: phase pill (A-E) + last-event pill (Spring / SC /
// SOS / etc.) + alignment score badge. The phase tells WHERE in the
// schematic we are; the event tells WHAT just fired; the alignment
// score tells how well it matches the Elliott wave context.
function WyckoffStrip({
  phase,
  event,
  age,
  alignment,
}: {
  phase?: string | null;
  event?: string | null;
  age?: number | null;
  alignment?: number | null;
}) {
  if (!phase && !event) return null;
  const phaseColor: Record<string, string> = {
    a: "bg-rose-500/30 text-rose-200",      // capitulation / climax
    b: "bg-amber-500/20 text-amber-200",    // building cause
    c: "bg-amber-500/30 text-amber-100",    // final test (spring/utad)
    d: "bg-emerald-500/30 text-emerald-100", // confirmation
    e: "bg-emerald-500/40 text-emerald-50", // markup / markdown
  };
  const phaseLower = (phase ?? "").toLowerCase();
  const phaseCls =
    phaseColor[phaseLower] ?? "bg-zinc-700/40 text-zinc-100";
  // Strip the _bull / _bear suffix from event subkind for display.
  const eventLabel = (event ?? "").replace(/_(bull|bear)$/i, "").toUpperCase();
  const isBull = (event ?? "").endsWith("_bull");
  const eventCls = isBull
    ? "bg-emerald-500/20 text-emerald-200"
    : "bg-rose-500/20 text-rose-200";
  const alignCls =
    alignment != null && alignment >= 0.85
      ? "bg-emerald-500/40 text-emerald-100"
      : alignment != null && alignment >= 0.5
      ? "bg-amber-500/20 text-amber-200"
      : alignment != null && alignment > 0
      ? "bg-zinc-700/40 text-zinc-200"
      : "";
  return (
    <div className="mt-0.5 flex flex-wrap items-center gap-0.5 text-[8px]">
      {phase && (
        <span
          className={`rounded px-1 font-mono ${phaseCls}`}
          title={`Wyckoff Phase ${phase.toUpperCase()}`}
        >
          P{phase.toUpperCase()}
        </span>
      )}
      {event && (
        <span
          className={`rounded px-1 font-mono ${eventCls}`}
          title={`${event} (${age ?? "?"}m ago)`}
        >
          {eventLabel}
        </span>
      )}
      {alignment != null && alignment > 0 && (
        <span
          className={`rounded px-1 font-mono ${alignCls}`}
          title={`Wyckoff↔Elliott alignment ${(alignment * 100).toFixed(0)}%`}
        >
          ↔{(alignment * 100).toFixed(0)}
        </span>
      )}
    </div>
  );
}

// Setup pipeline lifecycle pill row. Surfaces every state count for
// (iq_d + iq_t) on this TF — operator sees the funnel at a glance.
function LifecycleStrip({ counts }: { counts: Record<string, number> }) {
  const states: Array<[string, string, string]> = [
    ["candidate", "C", "bg-zinc-700/40 text-zinc-300"],
    ["armed", "A", "bg-amber-500/30 text-amber-200"],
    ["active", "▶", "bg-emerald-500/30 text-emerald-200"],
    ["triggered", "T", "bg-emerald-600/40 text-emerald-100"],
    ["closed", "×", "bg-zinc-800 text-zinc-200"],
    ["rejected", "R", "bg-rose-500/30 text-rose-200"],
  ];
  const present = states.filter(([k]) => (counts[k] ?? 0) > 0);
  if (present.length === 0) return null;
  return (
    <div className="mt-0.5 flex flex-wrap gap-0.5 text-[8px]">
      {present.map(([k, label, cls]) => (
        <span key={k} className={`rounded px-1 font-mono ${cls}`} title={k}>
          {label} {counts[k]}
        </span>
      ))}
    </div>
  );
}

// Compact wave + branch indicator. Highlights "C" + flat_expanded
// etc. so the user spots non-zigzag corrections at a glance.
function WaveCell({ tf }: { tf: TfSnapshot }) {
  if (!tf.iq_state) {
    return <span className="text-zinc-200">—</span>;
  }
  const stateColor: Record<string, string> = {
    candidate: "text-zinc-100",
    tracking: "text-emerald-300",
    completed: "text-cyan-300",
    invalidated: "text-zinc-200",
  };
  const bc = (tf.primary_branch || "").replace(/^flat_/, "f.");
  return (
    <span className={`font-mono text-[10px] ${stateColor[tf.iq_state] ?? "text-zinc-300"}`}>
      {tf.iq_wave ?? "?"}
      {bc && <span className="ml-0.5 text-[8px] text-zinc-200">·{bc}</span>}
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
          <p className="text-[11px] text-zinc-200">
            Exchange · Market · Sembol kırılımında IQ dalgaları, Major Dip
            skorları, açık pozisyonlar ve onchain durum.
          </p>
        </div>
        <div className="text-[10px] text-zinc-200">
          {data && new Date(data.generated_at).toLocaleString("tr-TR")}
        </div>
      </div>

      {isLoading && <div className="text-zinc-200">Yükleniyor…</div>}

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
              <span className="text-[10px] text-zinc-200">{open ? "−" : "+"} {rows.length}</span>
            </button>
            {open && (
              <div className="overflow-x-auto rounded border border-zinc-800">
                <table className="w-full text-[11px]">
                  <thead className="bg-zinc-900/40">
                    <tr className="text-left text-[9px] uppercase tracking-wider text-zinc-200">
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
                                <td key={tfKey} className="px-2 py-1 text-center text-zinc-500">
                                  —
                                </td>
                              );
                            }
                            // Choose dip or top breakdown based on
                            // higher composite score so the reader
                            // sees the dominant direction's channels.
                            const dipS = tf.major_dip_score ?? 0;
                            const topS = tf.major_top_score ?? 0;
                            const dominant: "dip" | "top" = topS > dipS ? "top" : "dip";
                            return (
                              <td key={tfKey} className="px-2 py-1 align-top">
                                <div className="flex items-center justify-between gap-1">
                                  <WaveCell tf={tf} />
                                  <div className="flex flex-col items-end gap-0.5">
                                    <VerdictPill
                                      polarity="dip"
                                      prefix="↑"
                                      verdict={tf.major_dip_verdict}
                                      score={tf.major_dip_score}
                                    />
                                    <VerdictPill
                                      polarity="top"
                                      prefix="↓"
                                      verdict={tf.major_top_verdict}
                                      score={tf.major_top_score}
                                    />
                                  </div>
                                </div>
                                <ComponentBars
                                  comps={
                                    dominant === "dip"
                                      ? tf.dip_components
                                      : tf.top_components
                                  }
                                />
                                <WyckoffStrip
                                  phase={tf.wyckoff_phase}
                                  event={tf.wyckoff_last_event}
                                  age={tf.wyckoff_event_age_min}
                                  alignment={tf.wyckoff_alignment_score}
                                />
                                <LifecycleStrip counts={tf.setup_lifecycle ?? {}} />
                              </td>
                            );
                          })}
                          <td className="px-2 py-1 text-right font-mono">
                            {row.open_positions || <span className="text-zinc-500">—</span>}
                          </td>
                          <td className="px-2 py-1 text-right font-mono text-zinc-300">
                            {fmtNotional(row.aggregate_notional_usd)}
                          </td>
                          <td className="px-2 py-1 text-right font-mono text-[10px]">
                            {row.onchain_score != null ? (
                              <div className="flex flex-col items-end gap-0.5">
                                <span
                                  className={
                                    row.onchain_direction === "strong_buy"
                                      ? "rounded bg-emerald-500/30 px-1 text-emerald-200"
                                      : row.onchain_direction === "buy"
                                      ? "rounded bg-emerald-500/20 px-1 text-emerald-300"
                                      : row.onchain_direction === "strong_sell"
                                      ? "rounded bg-rose-500/30 px-1 text-rose-200"
                                      : row.onchain_direction === "sell"
                                      ? "rounded bg-rose-500/20 px-1 text-rose-300"
                                      : "rounded bg-zinc-700/40 px-1 text-zinc-100"
                                  }
                                >
                                  {row.onchain_direction ?? "—"} {(row.onchain_score * 100).toFixed(0)}%
                                </span>
                                {row.onchain_kind && (
                                  <span className="text-cyan-400">
                                    {row.onchain_kind}
                                    {ageMin != null && (
                                      <span className="ml-1 text-zinc-200">{ageMin}m</span>
                                    )}
                                  </span>
                                )}
                              </div>
                            ) : row.onchain_kind ? (
                              <span className="text-cyan-400">
                                {row.onchain_kind}
                                {ageMin != null && (
                                  <span className="ml-1 text-zinc-200">{ageMin}m</span>
                                )}
                              </span>
                            ) : (
                              <span className="text-zinc-500">—</span>
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
        <div className="rounded border border-zinc-800 bg-zinc-900/40 p-3 text-zinc-200">
          Aktif sembol yok. Engine Symbols sayfasından sembol etkinleştir.
        </div>
      )}
    </div>
  );
}
