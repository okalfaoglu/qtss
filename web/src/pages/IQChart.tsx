// FAZ 25 PR-25A — dedicated demo page for the IQ-D / IQ-T predictive
// layer. For now the chart body is the same LuxAlgoChart component
// (so the existing toolbar, slot toggles, indicator overlays etc. all
// work) — only difference is a top banner explaining the new N/F/X
// markers and a recent-detections sidebar that pulls /v2/elliott-early
// independently. Future PR-25B will extend this page with the IQ-D
// structure tracker overlay.

import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { LuxAlgoChart } from "./LuxAlgoChart";
import { WaveBarsPanel } from "./WaveBarsPanel";
import { apiFetch } from "../lib/api";
import { getToken } from "../lib/auth";

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
    abc_nascent: { letter: "a?", bg: "bg-amber-500/20", fg: "text-amber-300" },
    abc_forming: { letter: "ab", bg: "bg-orange-500/20", fg: "text-orange-300" },
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

  // FAZ 25 PR-25B — IQ structure tracker rows (current wave, state,
  // symbol-lock). Read-only — the worker writes them; we surface them.
  type IqAnchor = { wave_label?: string; price?: number; time?: string };
  type IqStructure = {
    id: string;
    slot: number;
    direction: number;
    state: string;
    current_wave: string;
    current_stage: string;
    structure_anchors: IqAnchor[];
    started_at: string;
    last_advanced_at: string;
    invalidation_reason?: string | null;
  };
  type IqLock = { locked_at: string; reason?: string | null };
  type IqResp = {
    structures: IqStructure[];
    lock: IqLock | null;
  };
  const iq = useQuery<IqResp>({
    queryKey: ["iq-structures", symbol, tf],
    queryFn: () =>
      apiFetch(
        `/v2/iq-structures/binance/${symbol}/${tf}?segment=futures&limit=10`
      ),
    refetchInterval: 30_000,
  });
  const structures = iq.data?.structures ?? [];
  const lock = iq.data?.lock ?? null;

  // FAZ 25 PR-25C/D — iq_d / iq_t setups for the same symbol+tf.
  // Surfaced in the sidebar so the user can verify entry/SL/TP and
  // the parent → child link without leaving the chart.
  type IqSetup = {
    id: string;
    profile: "iq_d" | "iq_t" | string;
    direction: "long" | "short" | string;
    state: string;
    entry_price?: number | null;
    entry_sl?: number | null;
    target_ref?: number | null;
    parent_setup_id?: string | null;
    created_at: string;
    raw_meta?: { entry_tier?: string; parent_current_wave?: string; child_tf?: string };
  };
  type IqSetupsResp = { setups: IqSetup[] };
  const iqSetups = useQuery<IqSetupsResp>({
    queryKey: ["iq-setups", symbol, tf],
    queryFn: () =>
      apiFetch(
        `/v2/iq-setups/binance/${symbol}/${tf}?segment=futures&limit=20`
      ),
    refetchInterval: 30_000,
  });
  const setups = iqSetups.data?.setups ?? [];

  // FAZ 25.3.D — Major Dip composite score breakdown panel. The
  // 8-component score (research doc §VIII + §XII) tells the user
  // exactly WHY a setup did or didn't arm. IQ-D/IQ-T setup creation
  // is gated on `min_score_for_setup` (default 0.55); this panel
  // visualises which channels lit up vs which are still stub-zero.
  type MajorDipResp = {
    rows: Array<{
      candidate_bar: number;
      candidate_time: string;
      candidate_price: number;
      score: number;
      components: Record<string, number | null>;
      verdict: string;
    }>;
  };
  const majorDip = useQuery<MajorDipResp>({
    queryKey: ["major-dip", symbol, tf],
    queryFn: () =>
      apiFetch(`/v2/major-dip/binance/${symbol}/${tf}?segment=futures&limit=1`),
    refetchInterval: 30_000,
  });
  const dipRow = majorDip.data?.rows?.[0];

  return (
    <aside className="hidden w-[300px] shrink-0 overflow-y-auto border-l border-zinc-800 bg-zinc-950/40 p-3 text-xs xl:block">
      {/* FAZ 25 PR-25B — IQ-D structure summary at the top so the user
          immediately sees the active wave / state for this chart. */}
      <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
        {symbol} {tf} — IQ Structure
      </h3>
      {lock && (
        <div className="mb-2 rounded border border-amber-700/40 bg-amber-500/10 px-2 py-1 text-[10px] text-amber-200">
          🔒 LOCK · {lock.reason || "structure invalidated"}
          <div className="text-amber-400/60">
            {new Date(lock.locked_at).toLocaleString("tr-TR")}
          </div>
        </div>
      )}
      {structures.length === 0 && (
        <div className="mb-3 text-[10px] text-zinc-500">No tracked structures yet.</div>
      )}
      {structures.slice(0, 5).map((s) => {
        const dirChar = s.direction === 1 ? "▲" : "▼";
        const dirColor = s.direction === 1 ? "text-emerald-400" : "text-rose-400";
        const stateColor: Record<string, string> = {
          candidate: "text-zinc-400",
          tracking: "text-emerald-300",
          completed: "text-cyan-300",
          invalidated: "text-zinc-500 line-through",
        };
        return (
          <div
            key={s.id}
            className="mb-2 rounded bg-zinc-900/60 px-2 py-1 text-[10px]"
          >
            <div className="flex items-center justify-between">
              <span className={`font-mono ${dirColor}`}>{dirChar} Z{s.slot + 1}</span>
              <span className={`font-semibold ${stateColor[s.state] ?? ""}`}>
                {s.state}
              </span>
            </div>
            <div className="mt-0.5 flex items-center justify-between text-zinc-300">
              <span>{s.current_wave} · {s.current_stage}</span>
              <span className="text-zinc-500">
                {new Date(s.last_advanced_at).toLocaleTimeString("tr-TR", {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
            </div>
            {s.invalidation_reason && (
              <div className="mt-0.5 truncate text-[9px] text-rose-400/70">
                {s.invalidation_reason}
              </div>
            )}
          </div>
        );
      })}

      {/* FAZ 25.3.D — Major Dip composite score panel. Per-component
          breakdown so the operator sees exactly which channels are
          contributing (vs which are stub zero). */}
      <h3 className="mb-2 mt-3 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
        {symbol} {tf} — Major Dip
      </h3>
      {!dipRow && (
        <div className="mb-2 text-[10px] text-zinc-500">No score yet.</div>
      )}
      {dipRow && (
        <div className="mb-2 rounded bg-zinc-900/60 px-2 py-1 text-[10px]">
          <div className="flex items-center justify-between">
            <span className="font-mono text-zinc-300">
              @ {dipRow.candidate_price.toFixed(2)}
            </span>
            <span
              className={
                dipRow.verdict === "very_high"
                  ? "rounded bg-emerald-500/30 px-1 font-bold text-emerald-200"
                  : dipRow.verdict === "high"
                  ? "rounded bg-emerald-500/20 px-1 text-emerald-300"
                  : dipRow.verdict === "developing"
                  ? "rounded bg-amber-500/20 px-1 text-amber-300"
                  : "rounded bg-zinc-700/40 px-1 text-zinc-400"
              }
            >
              {dipRow.verdict} {(dipRow.score * 100).toFixed(0)}%
            </span>
          </div>
          {/* Per-component bars. Each row paints a horizontal sparkline
              of the component score (0..1) so the operator sees which
              channels are contributing at a glance. Stub components
              that always return 0 stay greyed out. */}
          <div className="mt-1 space-y-0.5">
            {[
              ["structural_completion", "Yapısal", 0.20],
              ["fib_retrace_quality", "Fib Zone", 0.15],
              ["volume_capitulation", "Vol Capit", 0.15],
              ["cvd_divergence", "CVD Div", 0.10],
              ["indicator_alignment", "RSI/MACD", 0.10],
              ["sentiment_extreme", "Sentiment", 0.10],
              ["multi_tf_confluence", "Multi-TF", 0.10],
              ["funding_oi_signals", "Funding/OI", 0.10],
            ].map(([key, label, w]) => {
              const v = (dipRow.components[key as string] as number) ?? 0;
              const widthPct = Math.max(0, Math.min(1, v)) * 100;
              const isStub = v === 0;
              return (
                <div key={key as string} className="flex items-center gap-1">
                  <span className="w-16 truncate text-[8px] text-zinc-500">
                    {label}
                  </span>
                  <div className="relative h-1.5 flex-1 overflow-hidden rounded-sm bg-zinc-800">
                    <div
                      className={
                        isStub
                          ? "h-full bg-zinc-700"
                          : v > 0.7
                          ? "h-full bg-emerald-500/70"
                          : v > 0.3
                          ? "h-full bg-amber-500/70"
                          : "h-full bg-rose-500/50"
                      }
                      style={{ width: `${widthPct}%` }}
                    />
                  </div>
                  <span className="w-7 text-right font-mono text-[8px] text-zinc-400">
                    {v.toFixed(2)}
                  </span>
                  <span className="w-7 text-right text-[8px] text-zinc-600">
                    ×{(w as number).toFixed(2)}
                  </span>
                </div>
              );
            })}
          </div>
          <div className="mt-1 text-[8px] text-zinc-500">
            {new Date(dipRow.candidate_time).toLocaleString("tr-TR", {
              month: "short",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            })}
          </div>
        </div>
      )}

      {/* IQ-D / IQ-T setups for this symbol+tf. Active rows surface
          first; closed rows are filtered server-side by default. */}
      <h3 className="mb-2 mt-3 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
        {symbol} {tf} — IQ Setups
      </h3>
      {setups.length === 0 && (
        <div className="mb-2 text-[10px] text-zinc-500">No active IQ setups.</div>
      )}
      {setups.slice(0, 8).map((s) => {
        const dirArrow = s.direction === "long" ? "▲" : "▼";
        const dirColor = s.direction === "long" ? "text-emerald-400" : "text-rose-400";
        const profileBg = s.profile === "iq_d"
          ? "bg-indigo-500/20 text-indigo-300"
          : "bg-amber-500/20 text-amber-300";
        const tier = s.raw_meta?.entry_tier ?? s.raw_meta?.parent_current_wave;
        return (
          <div
            key={s.id}
            className="mb-1 rounded bg-zinc-900/60 px-2 py-1 text-[10px]"
          >
            <div className="flex items-center justify-between">
              <span className={`rounded px-1 font-mono uppercase ${profileBg}`}>
                {s.profile}
              </span>
              <span className={`font-mono ${dirColor}`}>
                {dirArrow} {tier ?? "?"}
              </span>
              <span className="text-zinc-500">{s.state}</span>
            </div>
            {s.entry_price && s.entry_sl && s.target_ref && (
              <div className="mt-0.5 grid grid-cols-3 gap-1 font-mono text-[9px]">
                <span className="text-zinc-400">
                  E <span className="text-zinc-200">{s.entry_price.toFixed(2)}</span>
                </span>
                <span className="text-rose-400">
                  SL <span>{s.entry_sl.toFixed(2)}</span>
                </span>
                <span className="text-emerald-400">
                  TP <span>{s.target_ref.toFixed(2)}</span>
                </span>
              </div>
            )}
            {s.parent_setup_id && (
              <div className="mt-0.5 truncate text-[9px] text-zinc-500">
                ↑ parent {s.parent_setup_id.slice(0, 8)}
              </div>
            )}
          </div>
        );
      })}

      <h3 className="mb-2 mt-3 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
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

  // FAZ 25 PR-25C/D — pull active iq_d / iq_t setups for THIS symbol
  // + tf so we can paint entry / SL / TP price lines on the chart
  // and the right-sidebar list. iq_d in indigo, iq_t in amber so
  // the user can tell parent macro from child tactical at a glance.
  type IqOverlaySetup = {
    id: string;
    profile: "iq_d" | "iq_t" | string;
    direction: string;
    entry_price?: number | null;
    entry_sl?: number | null;
    target_ref?: number | null;
  };
  const iqOverlay = useQuery<{ setups: IqOverlaySetup[] }>({
    queryKey: ["iq-setups-overlay", ctx.symbol, ctx.tf],
    queryFn: () =>
      apiFetch(
        `/v2/iq-setups/${ctx.exchange}/${ctx.symbol}/${ctx.tf}?segment=${ctx.segment}&limit=20`
      ),
    refetchInterval: 30_000,
  });
  // FAZ 25 PR-25F — real-time SSE. PostgreSQL pg_notify on
  // qtss_iq_changed (and the existing qtss_market_bars_gap_filled)
  // is forwarded by /v2/iq-stream as Server-Sent Events. We use each
  // event to invalidate the react-query caches that depend on it,
  // collapsing the worst-case GUI latency from the 30s polling tick
  // down to the network round-trip.
  const queryClient = useQueryClient();
  useEffect(() => {
    // SSE endpoint runs unauthenticated (see server-side note in
    // qtss-api/src/routes/mod.rs); no payload PII so the privacy bar
    // is low. We still keep getToken() so a future signed-token
    // upgrade is a one-line change here.
    const _token = getToken();
    if (!_token) return; // skip SSE if user is not signed in at all
    const url =
      `/api/v1/v2/iq-stream/${ctx.exchange}/${ctx.symbol}/${ctx.tf}` +
      `?segment=${ctx.segment}`;
    let es: EventSource | null = null;
    try {
      es = new EventSource(url);
    } catch {
      return;
    }
    const onMsg = () => {
      // Invalidate every cache the chart paints from. cheap enough
      // even at high-frequency events because the queries still
      // throttle at the network layer.
      queryClient.invalidateQueries({ queryKey: ["iq-structures"] });
      queryClient.invalidateQueries({ queryKey: ["iq-setups"] });
      queryClient.invalidateQueries({ queryKey: ["iq-setups-overlay"] });
      queryClient.invalidateQueries({ queryKey: ["iq-chart"] });
      queryClient.invalidateQueries({ queryKey: ["elliott-early"] });
      queryClient.invalidateQueries({ queryKey: ["elliott"] });
    };
    es.addEventListener("qtss_iq_changed", onMsg);
    es.addEventListener("qtss_market_bars_gap_filled", onMsg);
    return () => {
      es?.close();
    };
  }, [ctx.exchange, ctx.segment, ctx.symbol, ctx.tf, queryClient]);

  const priceLineOverlays = useMemo(() => {
    const out: Array<{
      price: number;
      color: string;
      title: string;
      lineWidth?: number;
      lineStyle?: "solid" | "dashed" | "dotted";
    }> = [];
    for (const s of iqOverlay.data?.setups ?? []) {
      const isD = s.profile === "iq_d";
      const baseColor = isD ? "#818cf8" : "#fbbf24"; // indigo / amber
      const tag = s.profile.toUpperCase();
      if (s.entry_price && Number.isFinite(s.entry_price)) {
        out.push({
          price: s.entry_price as number,
          color: baseColor,
          lineWidth: 1,
          lineStyle: "solid",
          title: `${tag} entry`,
        });
      }
      if (s.entry_sl && Number.isFinite(s.entry_sl)) {
        out.push({
          price: s.entry_sl as number,
          color: "#f87171",                       // rose-400
          lineWidth: 1,
          lineStyle: "dashed",
          title: `${tag} SL`,
        });
      }
      if (s.target_ref && Number.isFinite(s.target_ref)) {
        out.push({
          price: s.target_ref as number,
          color: "#34d399",                       // emerald-400
          lineWidth: 1,
          lineStyle: "dashed",
          title: `${tag} TP`,
        });
      }
    }
    return out;
  }, [iqOverlay.data]);

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
                onlyLatestMotive: false, // show all motives, not just the latest
                                         // — important for spotting newly-formed
                                         // patterns and in-progress ABCs
                priceLineOverlays,
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
