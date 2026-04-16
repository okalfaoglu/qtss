// Wyckoff Setups — Faz 10.
//
// Dedicated feed for the Wyckoff Setup Engine. Hits
// `/v2/wyckoff/setups` which pre-applies `alt_type LIKE 'wyckoff_%'`.
// Adds filters the shared Setups page doesn't expose: mode (dry/live/
// backtest), timeframe (1h/4h), alt_type (spring/ut/lps/…).
//
// The detail pane surfaces what makes Wyckoff signals unique:
//   * TP ladder (from `tp_ladder`)
//   * L1 classical audit (`wyckoff_classic`: range, pnf_target, climax)
//   * Composite score breakdown (`raw_meta.score_breakdown`)
//
// CLAUDE.md #1 — small helpers + lookup maps, no central match arms.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { SetupEntry, SetupEventsResponse } from "../lib/types";

// --- lookup tables (CLAUDE.md #1) ----------------------------------------

const MODES = ["", "dry", "live", "backtest"];
const STATES = ["", "armed", "active", "closed", "rejected", "invalidated"];
const TIMEFRAMES = ["", "1h", "4h"];
const ALT_TYPES = [
  "",
  "wyckoff_spring",
  "wyckoff_ut",
  "wyckoff_utad",
  "wyckoff_lps",
  "wyckoff_buec",
  "wyckoff_lpsy",
  "wyckoff_ice_retest",
  "wyckoff_jac",
];

// P7.6 — close_reason colour map. Severity ordered: structural > tactical
// > benign so the eye picks the worst failure first in the Plan panel.
const CLOSE_REASON_BADGE: Record<string, string> = {
  structural_invalidated:
    "bg-red-500/20 text-red-300 border-red-500/40",
  sl_breach:
    "bg-amber-500/15 text-amber-300 border-amber-500/40",
  tp_hit:
    "bg-emerald-500/15 text-emerald-300 border-emerald-500/40",
  time_stop:
    "bg-zinc-700/30 text-zinc-300 border-zinc-600/40",
};

const STATE_BADGE: Record<string, string> = {
  armed: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  active: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  closed: "bg-zinc-700/40 text-zinc-400 border-zinc-600/40",
  rejected: "bg-red-500/15 text-red-300 border-red-500/30",
  invalidated: "bg-red-500/20 text-red-300 border-red-500/40",
};

const PROFILE_BADGE: Record<string, string> = {
  t: "bg-orange-500/15 text-orange-300 border-orange-500/30",
  q: "bg-blue-500/15 text-blue-300 border-blue-500/30",
  d: "bg-purple-500/15 text-purple-300 border-purple-500/30",
};

const DIRECTION_COLOR: Record<string, string> = {
  long: "text-emerald-300",
  short: "text-red-300",
  neutral: "text-zinc-400",
};

// `wyckoff_spring` → `SPRING`. Keeps table compact.
function shortAlt(alt: string | null): string {
  if (!alt) return "—";
  return alt.replace(/^wyckoff_/, "").toUpperCase();
}

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtNum(v: number | null | undefined, digits = 4): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  return Number(v).toFixed(digits);
}

function badge(map: Record<string, string>, key: string): string {
  return map[key] ?? "bg-zinc-800/40 text-zinc-300 border-zinc-700";
}

// --- response shape -------------------------------------------------------

interface WyckoffSetupsResponse {
  generated_at: string;
  count: number;
  entries: SetupEntry[];
}

// --- row component --------------------------------------------------------

function Row({
  s,
  onSelect,
  selected,
}: {
  s: SetupEntry;
  onSelect: (id: string) => void;
  selected: boolean;
}) {
  const score = readScore(s.raw_meta);
  return (
    <tr
      onClick={() => onSelect(s.id)}
      className={`cursor-pointer border-b border-zinc-800/60 text-xs hover:bg-zinc-800/30 ${
        selected ? "bg-emerald-500/5" : ""
      }`}
    >
      <td className="px-2 py-1.5 font-mono text-zinc-300">{s.symbol}</td>
      <td className="px-2 py-1.5 text-zinc-500">{s.timeframe}</td>
      <td className="px-2 py-1.5 font-mono text-[10px] text-amber-300">
        {shortAlt(s.alt_type)}
      </td>
      <td className="px-2 py-1.5">
        <span
          className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${badge(PROFILE_BADGE, s.profile)}`}
        >
          {s.profile}
        </span>
      </td>
      <td className="px-2 py-1.5">
        <span
          className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${badge(STATE_BADGE, s.state)}`}
        >
          {s.state}
        </span>
      </td>
      <td
        className={`px-2 py-1.5 uppercase ${DIRECTION_COLOR[s.direction] ?? "text-zinc-400"}`}
      >
        {s.direction}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-200">
        {fmtNum(s.entry_price)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
        {fmtNum(s.entry_sl)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
        {fmtNum(s.target_ref)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-200">
        {score !== null ? score.toFixed(1) : "—"}
      </td>
      <td className="px-2 py-1.5 text-zinc-600">{fmtTs(s.updated_at)}</td>
    </tr>
  );
}

// --- detail pane helpers --------------------------------------------------

type Rec = Record<string, unknown>;
function asObj(v: unknown): Rec | null {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Rec) : null;
}

function readScore(raw: unknown): number | null {
  const r = asObj(raw);
  const s = r?.composite_score;
  return typeof s === "number" ? s : null;
}

interface TpRung {
  price: number;
  pct: number;
  label?: string;
}
function readTpLadder(raw: unknown): TpRung[] {
  const r = asObj(raw);
  const ladder = r?.tp_ladder ?? asObj(r?.plan)?.tp_ladder;
  if (!Array.isArray(ladder)) return [];
  const out: TpRung[] = [];
  for (const t of ladder) {
    const o = asObj(t);
    if (!o) continue;
    const price = typeof o.price === "number" ? o.price : null;
    const pct = typeof o.close_pct === "number" ? o.close_pct : null;
    if (price === null || pct === null) continue;
    const rung: TpRung = { price, pct };
    if (typeof o.label === "string") rung.label = o.label;
    out.push(rung);
  }
  return out;
}

function readScoreBreakdown(raw: unknown): Rec | null {
  const r = asObj(raw);
  return asObj(r?.score_breakdown);
}

function readClassic(raw: unknown): Rec | null {
  // `wyckoff_classic` is a sibling JSONB column on the setup; the API
  // currently bundles setup rows without it, so read from raw_meta.plan
  // fallback. We still surface `range_top/bottom/pnf_target` from meta.
  const r = asObj(raw);
  return asObj(r?.wyckoff_classic) ?? asObj(r?.classic) ?? null;
}

function readNum(rec: Rec | null, key: string): number | null {
  if (!rec) return null;
  const v = rec[key];
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

function readStr(rec: Rec | null, key: string): string | null {
  if (!rec) return null;
  const v = rec[key];
  return typeof v === "string" && v.length > 0 ? v : null;
}

// P7.4 — `tp_source` travels either in classic or in raw_meta.plan.
function readTpSource(raw: unknown): string | null {
  const r = asObj(raw);
  return (
    readStr(asObj(r?.plan), "tp_source") ??
    readStr(asObj(r?.wyckoff_classic), "tp_source") ??
    readStr(r, "tp_source")
  );
}

// --- detail pane ----------------------------------------------------------

function Detail({ entry }: { entry: SetupEntry }) {
  const ladder = readTpLadder(entry.raw_meta);
  const breakdown = readScoreBreakdown(entry.raw_meta);
  const classic = readClassic(entry.raw_meta);
  const score = readScore(entry.raw_meta);

  const events = useQuery({
    queryKey: ["v2", "setup", entry.id, "events"],
    queryFn: () =>
      apiFetch<SetupEventsResponse>(`/v2/setups/${entry.id}/events`),
    refetchInterval: 5_000,
  });

  return (
    <div className="space-y-3">
      <div className="border-b border-zinc-800 pb-2">
        <div className="text-sm font-semibold text-zinc-100">
          {entry.symbol}
          <span className="ml-2 text-xs text-zinc-500">
            · {entry.timeframe} · {shortAlt(entry.alt_type)} · profile{" "}
            {entry.profile.toUpperCase()}
          </span>
        </div>
        <div className="mt-1 flex flex-wrap gap-2 text-[10px] text-zinc-500">
          <span>{entry.exchange}</span>
          <span>{entry.venue_class}</span>
          {score !== null && (
            <span className="text-amber-300">score {score.toFixed(1)}</span>
          )}
        </div>
      </div>

      <section>
        <div className="mb-1 text-[10px] uppercase tracking-wide text-zinc-500">
          Plan
        </div>
        <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px]">
          <div className="text-zinc-500">Entry</div>
          <div className="text-right font-mono text-zinc-200">
            {fmtNum(entry.entry_price)}
          </div>
          <div className="text-zinc-500">Tight SL</div>
          <div className="text-right font-mono text-zinc-300">
            {fmtNum(entry.entry_sl)}
          </div>
          {/* P7.3 — structural (wide) SL lives in classic payload. */}
          {readNum(classic, "sl_wide") !== null && (
            <>
              <div className="text-zinc-500">Structural SL</div>
              <div className="text-right font-mono text-red-300/80">
                {fmtNum(readNum(classic, "sl_wide"))}
              </div>
            </>
          )}
          <div className="text-zinc-500">Target ref</div>
          <div className="text-right font-mono text-zinc-300">
            {fmtNum(entry.target_ref)}
          </div>
          {/* P7.4 — which TP engine produced the ladder. */}
          {readTpSource(entry.raw_meta) && (
            <>
              <div className="text-zinc-500">TP source</div>
              <div className="text-right font-mono text-sky-300/80">
                {readTpSource(entry.raw_meta)}
              </div>
            </>
          )}
          <div className="text-zinc-500">Risk %</div>
          <div className="text-right font-mono text-zinc-300">
            {fmtNum(entry.risk_pct, 2)}
          </div>
          {/* Trigger bar summary (P7.5 gate audit). */}
          {readStr(classic, "trigger_event") && (
            <>
              <div className="text-zinc-500">Trigger</div>
              <div className="text-right font-mono text-amber-300/80">
                {readStr(classic, "trigger_event")}
                {readNum(classic, "trigger_price") !== null &&
                  ` @ ${fmtNum(readNum(classic, "trigger_price"))}`}
              </div>
            </>
          )}
          {entry.close_reason && (
            <>
              <div className="text-zinc-500">Close</div>
              <div className="text-right">
                <span
                  className={`rounded border px-1.5 py-0.5 font-mono text-[10px] uppercase ${badge(CLOSE_REASON_BADGE, entry.close_reason)}`}
                >
                  {entry.close_reason}
                </span>
                <span className="ml-2 font-mono text-zinc-400">
                  @ {fmtNum(entry.close_price)}
                </span>
              </div>
            </>
          )}
        </div>
      </section>

      {ladder.length > 0 && (
        <section>
          <div className="mb-1 text-[10px] uppercase tracking-wide text-zinc-500">
            TP Ladder
          </div>
          <table className="w-full text-[11px]">
            <thead className="text-[10px] uppercase text-zinc-500">
              <tr>
                <th className="text-left">#</th>
                <th className="text-left">Label</th>
                <th className="text-right">Price</th>
                <th className="text-right">Close%</th>
              </tr>
            </thead>
            <tbody>
              {ladder.map((tp, i) => (
                <tr key={i} className="border-t border-zinc-800/40">
                  <td className="py-0.5 text-zinc-500">{i + 1}</td>
                  <td className="py-0.5 text-zinc-300">{tp.label ?? "—"}</td>
                  <td className="py-0.5 text-right font-mono text-zinc-200">
                    {fmtNum(tp.price)}
                  </td>
                  <td className="py-0.5 text-right font-mono text-zinc-400">
                    {(tp.pct * 100).toFixed(0)}%
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {breakdown && (
        <section>
          <div className="mb-1 text-[10px] uppercase tracking-wide text-zinc-500">
            Score Breakdown
          </div>
          <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 text-[11px]">
            {Object.entries(breakdown).map(([k, v]) => (
              <div key={k} className="contents">
                <div className="text-zinc-500">{k}</div>
                <div className="text-right font-mono text-zinc-300">
                  {typeof v === "number" ? v.toFixed(2) : String(v)}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {classic && (
        <section>
          <div className="mb-1 text-[10px] uppercase tracking-wide text-zinc-500">
            L1 Classical
          </div>
          <pre className="overflow-x-auto rounded bg-zinc-950/60 p-2 text-[10px] text-zinc-400">
            {JSON.stringify(classic, null, 2)}
          </pre>
        </section>
      )}

      <section>
        <div className="mb-1 text-[10px] uppercase tracking-wide text-zinc-500">
          Events
        </div>
        {events.isLoading && (
          <div className="text-xs text-zinc-500">Loading…</div>
        )}
        {events.data?.events.length === 0 && (
          <div className="text-xs text-zinc-500">No events yet.</div>
        )}
        <div className="space-y-1">
          {events.data?.events.map((ev) => (
            <div
              key={ev.id}
              className="rounded border border-zinc-800 bg-zinc-900/40 p-1.5 text-[11px]"
            >
              <div className="flex items-baseline justify-between">
                <span className="font-mono text-zinc-200">{ev.event_type}</span>
                <span className="text-[10px] text-zinc-500">
                  {fmtTs(ev.created_at)}
                </span>
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

// --- page -----------------------------------------------------------------

export function WyckoffSetups() {
  const [mode, setMode] = useState("dry");
  const [state, setState] = useState("");
  const [timeframe, setTimeframe] = useState("");
  const [symbol, setSymbol] = useState("");
  const [selected, setSelected] = useState<string | null>(null);

  const query = useQuery({
    queryKey: ["v2", "wyckoff", "setups", { mode, state, timeframe, symbol }],
    queryFn: () => {
      const p = new URLSearchParams();
      if (mode) p.set("mode", mode);
      if (state) p.set("state", state);
      if (timeframe) p.set("timeframe", timeframe);
      if (symbol) p.set("symbol", symbol);
      p.set("limit", "200");
      return apiFetch<WyckoffSetupsResponse>(
        `/v2/wyckoff/setups?${p.toString()}`,
      );
    },
    refetchInterval: 5_000,
  });

  const entries = useMemo(() => query.data?.entries ?? [], [query.data]);
  const selectedEntry = useMemo(
    () => entries.find((e) => e.id === selected) ?? null,
    [entries, selected],
  );

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Wyckoff Signals — {entries.length} setup
        </div>
        <div className="text-xs text-zinc-500">
          {query.data ? `Generated at ${fmtTs(query.data.generated_at)}` : ""}
        </div>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
        <select
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {MODES.map((m) => (
            <option key={m} value={m}>
              {m ? `mode: ${m}` : "all modes"}
            </option>
          ))}
        </select>
        <select
          value={state}
          onChange={(e) => setState(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {STATES.map((s) => (
            <option key={s} value={s}>
              {s || "all states"}
            </option>
          ))}
        </select>
        <select
          value={timeframe}
          onChange={(e) => setTimeframe(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {TIMEFRAMES.map((t) => (
            <option key={t} value={t}>
              {t || "all TFs"}
            </option>
          ))}
        </select>
        <input
          value={symbol}
          onChange={(e) => setSymbol(e.target.value.toUpperCase())}
          placeholder="SYMBOL (exact)"
          className="w-40 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 font-mono text-zinc-200 placeholder:text-zinc-600"
        />
        <span className="text-[10px] text-zinc-600 self-center">
          alt types: {ALT_TYPES.filter(Boolean).map(shortAlt).join(" · ")}
        </span>
      </div>

      {query.isLoading && (
        <div className="text-sm text-zinc-400">Loading setups…</div>
      )}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}

      {!query.isLoading && entries.length === 0 && (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-6 text-sm text-zinc-500">
          No Wyckoff setups match the current filter.
        </div>
      )}

      {entries.length > 0 && (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <div className="lg:col-span-2 overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
            <table className="min-w-full">
              <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
                <tr>
                  <th className="px-2 py-2 text-left">Symbol</th>
                  <th className="px-2 py-2 text-left">TF</th>
                  <th className="px-2 py-2 text-left">Type</th>
                  <th className="px-2 py-2 text-left">Profile</th>
                  <th className="px-2 py-2 text-left">State</th>
                  <th className="px-2 py-2 text-left">Dir</th>
                  <th className="px-2 py-2 text-right">Entry</th>
                  <th className="px-2 py-2 text-right">SL</th>
                  <th className="px-2 py-2 text-right">Target</th>
                  <th className="px-2 py-2 text-right">Score</th>
                  <th className="px-2 py-2 text-left">Updated</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((s) => (
                  <Row
                    key={s.id}
                    s={s}
                    onSelect={setSelected}
                    selected={selected === s.id}
                  />
                ))}
              </tbody>
            </table>
          </div>

          <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
            {selectedEntry ? (
              <Detail entry={selectedEntry} />
            ) : (
              <div className="text-xs text-zinc-500">
                Select a Wyckoff setup to view plan & score breakdown.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
