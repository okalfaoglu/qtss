// FAZ 25 PR-25A — dedicated demo page for the IQ-D / IQ-T predictive
// layer. For now the chart body is the same LuxAlgoChart component
// (so the existing toolbar, slot toggles, indicator overlays etc. all
// work) — only difference is a top banner explaining the new N/F/X
// markers and a recent-detections sidebar that pulls /v2/elliott-early
// independently. Future PR-25B will extend this page with the IQ-D
// structure tracker overlay.

import { useQuery } from "@tanstack/react-query";

import { LuxAlgoChart } from "./LuxAlgoChart";
import { apiFetch } from "../lib/api";

type EarlyMarker = {
  slot: number;
  subkind: string;
  stage: string;
  direction: number;
  start_time: string;
  end_time: string;
  score: number;
  w3_extension: number;
  invalidation_price: number;
};
type EarlyResponse = {
  venue: string;
  symbol: string;
  timeframe: string;
  markers: EarlyMarker[];
};

function StageBadge({ stage }: { stage: string }) {
  const map: Record<string, { letter: string; bg: string; fg: string }> = {
    nascent: { letter: "N", bg: "bg-emerald-500/20", fg: "text-emerald-300" },
    forming: { letter: "F", bg: "bg-cyan-500/20", fg: "text-cyan-300" },
    extended: { letter: "X", bg: "bg-violet-500/20", fg: "text-violet-300" },
  };
  const s = map[stage] ?? { letter: "?", bg: "bg-zinc-500/20", fg: "text-zinc-300" };
  return (
    <span
      className={`inline-flex h-5 w-5 items-center justify-center rounded text-[11px] font-bold ${s.bg} ${s.fg}`}
    >
      {s.letter}
    </span>
  );
}

// Lightweight stats panel that reads /v2/elliott-early for the
// currently-viewed symbol+tf. We can't share state with the underlying
// LuxAlgoChart (no URL params right now), so we hard-code the demo
// pair (BTCUSDT 4h) here — operators can still click through any
// symbol on the chart toolbar; the sidebar just shows BTC stats as
// a quick cross-check that the writer is producing data.
function EarlyStatsPanel() {
  const { data, isLoading, isError } = useQuery<EarlyResponse>({
    queryKey: ["iq-chart", "early-stats", "BTCUSDT", "4h"],
    queryFn: () =>
      apiFetch(`/v2/elliott-early/binance/BTCUSDT/4h?segment=futures&limit=20`),
    refetchInterval: 15_000,
  });
  const markers = data?.markers ?? [];

  // Quick aggregation by stage so the panel header shows totals at a glance.
  const counts = markers.reduce(
    (acc, m) => {
      acc[m.stage] = (acc[m.stage] ?? 0) + 1;
      return acc;
    },
    {} as Record<string, number>
  );

  return (
    <aside className="hidden w-[300px] shrink-0 overflow-y-auto border-l border-zinc-800 bg-zinc-950/40 p-3 text-xs xl:block">
      <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
        BTCUSDT 4h — Early-Wave Activity
      </h3>
      <div className="mb-3 flex items-center gap-2 text-[11px]">
        <StageBadge stage="nascent" />
        <span className="text-zinc-300">{counts.nascent ?? 0}</span>
        <StageBadge stage="forming" />
        <span className="text-zinc-300">{counts.forming ?? 0}</span>
        <StageBadge stage="extended" />
        <span className="text-zinc-300">{counts.extended ?? 0}</span>
      </div>
      {isLoading && <div className="text-zinc-500">loading…</div>}
      {isError && (
        <div className="text-red-400">/v2/elliott-early failed — check API logs</div>
      )}
      {!isLoading && !isError && markers.length === 0 && (
        <div className="text-zinc-500">No early-wave detections yet.</div>
      )}
      <ul className="space-y-1">
        {markers.map((m, i) => {
          const dirArrow = m.direction === 1 ? "▲" : "▼";
          const dirColor =
            m.direction === 1 ? "text-emerald-400" : "text-rose-400";
          const time = new Date(m.end_time).toLocaleTimeString("tr-TR", {
            hour: "2-digit",
            minute: "2-digit",
          });
          return (
            <li
              key={`${m.subkind}-${m.end_time}-${i}`}
              className="flex items-center gap-2 rounded bg-zinc-900/60 px-2 py-1"
            >
              <StageBadge stage={m.stage} />
              <span className={`font-mono ${dirColor}`}>{dirArrow}</span>
              <span className="flex-1 truncate text-zinc-200">{m.subkind}</span>
              <span className="text-zinc-500">{time}</span>
              <span className="font-mono text-zinc-400">
                {m.score.toFixed(2)}
              </span>
            </li>
          );
        })}
      </ul>
      <p className="mt-3 text-[10px] leading-relaxed text-zinc-500">
        FAZ 25 PR-25A. Markers feed into the IQ-D candidate creator
        (PR-25C) once the structure tracker (PR-25B) lands.
      </p>
    </aside>
  );
}

export function IQChart() {
  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-zinc-800 bg-zinc-900/60 px-4 py-2">
        <div className="flex items-baseline gap-3">
          <h1 className="text-sm font-semibold text-emerald-400">IQ Chart</h1>
          <span className="text-[11px] text-zinc-500">
            FAZ 25 Predictive Layer · Elliott early-wave (N/F/X)
          </span>
        </div>
        <p className="mt-1 text-[11px] leading-tight text-zinc-400">
          <span className="font-mono text-emerald-300">N</span> nascent (4 pivots, W3 broke W1) ·
          <span className="ml-1 font-mono text-cyan-300">F</span> forming (5 pivots, W4 in) ·
          <span className="ml-1 font-mono text-violet-300">X</span> extended (full motive, tagged).
          Toggle <span className="font-mono">"N/F/X early"</span> in the chart toolbar to show / hide.
        </p>
      </div>
      <div className="flex flex-1 overflow-hidden">
        <div className="flex-1 overflow-hidden">
          <LuxAlgoChart />
        </div>
        <EarlyStatsPanel />
      </div>
    </div>
  );
}

export default IQChart;
