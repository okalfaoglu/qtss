// FAZ 25 PR-25A — dedicated demo page for the IQ-D / IQ-T predictive
// layer. For now the chart body is the same LuxAlgoChart component
// (so the existing toolbar, slot toggles, indicator overlays etc. all
// work) — only difference is a top banner explaining the new N/F/X
// markers and a recent-detections sidebar that pulls /v2/elliott-early
// independently. Future PR-25B will extend this page with the IQ-D
// structure tracker overlay.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { LuxAlgoChart } from "./LuxAlgoChart";
import { WaveBarsPanel } from "./WaveBarsPanel";
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
// currently-viewed symbol+tf. Symbol/tf are now controlled from
// IQChart so the panel re-fetches whenever the user flips TF in the
// chart toolbar.
function EarlyStatsPanel({
  symbol = "BTCUSDT",
  tf = "4h",
}: {
  symbol?: string;
  tf?: string;
}) {
  const { data, isLoading, isError } = useQuery<EarlyResponse>({
    queryKey: ["iq-chart", "early-stats", symbol, tf],
    queryFn: () =>
      apiFetch(`/v2/elliott-early/binance/${symbol}/${tf}?segment=futures&limit=20`),
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
        {symbol} {tf} — Early-Wave Activity
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
  // Shared chart context so the WaveBarsPanel below stays in sync
  // with the user's venue / symbol / TF picks in the LuxAlgoChart
  // toolbar above. LuxAlgoChart manages its own local state but pings
  // us via onContextChange whenever any of these flip.
  const [ctx, setCtx] = useState({
    exchange: "binance",
    segment: "futures",
    symbol: "BTCUSDT",
    tf: "4h",
  });

  return (
    <div
      className="flex flex-col"
      style={{ height: "calc(100dvh - 110px)" }}
    >
      <div className="flex flex-1 min-h-0">
        <div className="flex flex-1 min-h-0 flex-col">
          {/* Top half: existing OHLC chart with N/F/X early-wave toggle. */}
          <div className="flex-1 min-h-0 overflow-y-auto border-b-2 border-zinc-800">
            <LuxAlgoChart
              defaults={{
                showZigzag: false,
                showHarmonic: false,
                showRange: false,
                showGap: false,
                slotsEnabled: [true, true, true, true, true],
                embedded: true,
                onContextChange: setCtx,
              }}
            />
          </div>
          {/* Bottom half: pivot-based wave bars. */}
          <div className="flex-none border-t-2 border-zinc-800">
            <WaveBarsPanel
              exchange={ctx.exchange}
              segment={ctx.segment}
              symbol={ctx.symbol}
              tf={ctx.tf}
            />
          </div>
        </div>
        <EarlyStatsPanel symbol={ctx.symbol} tf={ctx.tf} />
      </div>
    </div>
  );
}

export default IQChart;
