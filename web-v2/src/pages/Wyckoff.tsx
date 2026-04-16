import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

// =========================================================================
// Types
// =========================================================================

interface WyckoffStructure {
  id: string;
  symbol: string;
  interval: string;
  exchange: string;
  schematic: string;
  current_phase: string;
  range_top: number | null;
  range_bottom: number | null;
  creek_level: number | null;
  ice_level: number | null;
  confidence: number | null;
  events_json: WyckoffEventEntry[];
  is_active: boolean;
  started_at: string;
  completed_at: string | null;
  failed_at: string | null;
  failure_reason: string | null;
}

interface WyckoffEventEntry {
  event: string;
  bar_index: number;
  price: number;
  score: number;
}

// =========================================================================
// Color maps
// =========================================================================

const PHASE_COLORS: Record<string, string> = {
  A: "bg-red-500/20 text-red-300 border-red-500/40",
  B: "bg-orange-500/20 text-orange-300 border-orange-500/40",
  C: "bg-yellow-500/20 text-yellow-300 border-yellow-500/40",
  D: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  E: "bg-blue-500/20 text-blue-300 border-blue-500/40",
};

const SCHEMATIC_COLORS: Record<string, string> = {
  accumulation: "text-emerald-400",
  distribution: "text-red-400",
  reaccumulation: "text-teal-400",
  redistribution: "text-rose-400",
};

// Wyckoff literature commits a schematic family (accum vs distrib) at
// Phase C — Spring for accumulation, UTAD for distribution. Before C the
// family call is provisional: only PS/BC/AR/ST evidence exists, which any
// later shakeout or upthrust can legitimately flip. Mirrors the backend
// `detector.wyckoff.schematic_lock_phase = C` guard (migration 0124) so
// the GUI stops announcing "DISTRIBUTION" on a Phase-A card with two AR
// events.
function isSchematicLocked(phase: string): boolean {
  const p = (phase ?? "").toUpperCase();
  return p === "C" || p === "D" || p === "E";
}

// Muted palette used when the schematic is still provisional — same hue
// family but desaturated so the operator can see the *bias* without
// mistaking it for a committed call.
const SCHEMATIC_PROVISIONAL_COLORS: Record<string, string> = {
  accumulation: "text-emerald-500/50",
  distribution: "text-red-500/50",
  reaccumulation: "text-teal-500/50",
  redistribution: "text-rose-500/50",
};

function schematicDisplay(schematic: string, phase: string): {
  label: string;
  className: string;
  provisional: boolean;
} {
  const locked = isSchematicLocked(phase);
  const key = (schematic ?? "").toLowerCase();
  if (!locked) {
    return {
      label: `RANGE · ${key}?`,
      className: SCHEMATIC_PROVISIONAL_COLORS[key] ?? "text-zinc-500",
      provisional: true,
    };
  }
  return {
    label: key.toUpperCase(),
    className: SCHEMATIC_COLORS[key] ?? "text-zinc-300",
    provisional: false,
  };
}

const PHASE_DESC: Record<string, string> = {
  A: "Stopping — SC/BC + AR + ST",
  B: "Building the Cause",
  C: "Test — Spring / UTAD",
  D: "Trend within Range — SOS/SOW + LPS/LPSY",
  E: "Trend out of Range",
};

const EVENT_ICONS: Record<string, string> = {
  SC: "🔴", BC: "🔴", AR: "🔵", ST: "🟡",
  UA: "🟠", "ST-B": "🟡",
  Spring: "🟢", UTAD: "🔴", Shakeout: "⚡",
  SOS: "💪", SOW: "📉", LPS: "🟢", LPSY: "🔴",
  JAC: "🚀", BreakOfIce: "❄️", BUEC: "↩️",
  SOT: "📊", Markup: "📈", Markdown: "📉",
};

// =========================================================================
// Components
// =========================================================================

function PhaseBadge({ phase }: { phase: string }) {
  return (
    <span className={`rounded border px-2 py-0.5 text-xs font-bold ${PHASE_COLORS[phase] ?? "bg-zinc-700 text-zinc-300"}`}>
      Phase {phase}
    </span>
  );
}

function ConfidenceBar({ value }: { value: number | null }) {
  const pct = value != null ? Math.round(value * 100) : 0;
  const color = pct >= 70 ? "bg-emerald-500" : pct >= 40 ? "bg-yellow-500" : "bg-red-500";
  return (
    <div className="flex items-center gap-2">
      <div className="h-2 w-20 rounded-full bg-zinc-800">
        <div className={`h-full rounded-full ${color}`} style={{ width: `${pct}%` }} />
      </div>
      <span className="text-xs text-zinc-400">{pct}%</span>
    </div>
  );
}

function EventTimeline({ events }: { events: WyckoffEventEntry[] }) {
  if (events.length === 0) return <div className="text-xs text-zinc-500">No events recorded</div>;
  return (
    <div className="space-y-1">
      {events.map((ev, i) => (
        <div key={i} className="flex items-center gap-2 text-xs">
          <span>{EVENT_ICONS[ev.event] ?? "⬜"}</span>
          <span className="font-mono font-bold text-zinc-200">{ev.event}</span>
          <span className="text-zinc-500">${ev.price.toFixed(2)}</span>
          <span className="text-zinc-600">score: {ev.score.toFixed(2)}</span>
        </div>
      ))}
    </div>
  );
}

function StatusBadge({ s }: { s: WyckoffStructure }) {
  if (s.is_active) {
    return <span className="rounded border border-emerald-500/40 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-bold text-emerald-300">ACTIVE</span>;
  }
  if (s.completed_at) {
    return <span className="rounded border border-blue-500/40 bg-blue-500/10 px-1.5 py-0.5 text-[10px] font-bold text-blue-300">COMPLETED</span>;
  }
  if (s.failed_at) {
    return <span className="rounded border border-red-500/40 bg-red-500/10 px-1.5 py-0.5 text-[10px] font-bold text-red-300">FAILED</span>;
  }
  return <span className="rounded border border-zinc-600 bg-zinc-800 px-1.5 py-0.5 text-[10px] font-bold text-zinc-400">CLOSED</span>;
}

function StructureCard({
  s,
  onSelect,
}: {
  s: WyckoffStructure;
  onSelect: (id: string) => void;
}) {
  return (
    <div
      className="cursor-pointer rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 transition hover:border-zinc-600"
      onClick={() => onSelect(s.id)}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm font-bold text-zinc-100">{s.symbol}</span>
          <span className="text-xs text-zinc-500">{s.interval}</span>
          <StatusBadge s={s} />
        </div>
        <PhaseBadge phase={s.current_phase} />
      </div>
      <div className="mt-2 flex items-center gap-3">
        {(() => {
          const d = schematicDisplay(s.schematic, s.current_phase);
          return (
            <span
              className={`text-sm font-semibold ${d.className}`}
              title={
                d.provisional
                  ? `Provisional bias — schematic commits at Phase C (UTAD/Spring). Current: Phase ${s.current_phase}.`
                  : undefined
              }
            >
              {d.label}
            </span>
          );
        })()}
        <ConfidenceBar value={s.confidence} />
      </div>
      <div className="mt-2 grid grid-cols-4 gap-2 text-xs text-zinc-400">
        <div>Range: {s.range_bottom?.toFixed(2)} – {s.range_top?.toFixed(2)}</div>
        {s.creek_level != null && <div>Creek: {s.creek_level.toFixed(2)}</div>}
        {s.ice_level != null && <div>Ice: {s.ice_level.toFixed(2)}</div>}
        <div>Events: {Array.isArray(s.events_json) ? s.events_json.length : 0}</div>
      </div>
      {s.failure_reason && (
        <div className="mt-2 rounded border border-red-900/50 bg-red-900/20 p-1.5 text-[11px] text-red-300">
          ⚠ {s.failure_reason}
        </div>
      )}
    </div>
  );
}

function StructureDetail({ id }: { id: string }) {
  const query = useQuery({
    queryKey: ["v2", "wyckoff", "structure", id],
    queryFn: () => apiFetch<WyckoffStructure>(`/v2/wyckoff/structure/${id}`),
    refetchInterval: 15_000,
  });
  const s = query.data;
  if (query.isLoading) return <div className="text-zinc-500">Loading...</div>;
  if (!s) return <div className="text-zinc-500">Structure not found</div>;

  const events: WyckoffEventEntry[] = Array.isArray(s.events_json) ? s.events_json : [];

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <span className="text-lg font-bold text-zinc-100">{s.symbol}</span>
        <span className="text-sm text-zinc-500">{s.interval}</span>
        <PhaseBadge phase={s.current_phase} />
        {(() => {
          const d = schematicDisplay(s.schematic, s.current_phase);
          return (
            <span
              className={`text-sm font-semibold ${d.className}`}
              title={
                d.provisional
                  ? `Provisional bias — schematic commits at Phase C (UTAD/Spring). Current: Phase ${s.current_phase}.`
                  : undefined
              }
            >
              {d.label}
            </span>
          );
        })()}
        {!isSchematicLocked(s.current_phase) && (
          <span className="rounded border border-zinc-700 bg-zinc-800/60 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-zinc-400">
            provisional
          </span>
        )}
      </div>

      {/* Phase description */}
      <div className="text-xs text-zinc-500">{PHASE_DESC[s.current_phase] ?? ""}</div>

      {/* Key levels */}
      <div className="grid grid-cols-2 gap-4 rounded border border-zinc-800 bg-zinc-900/40 p-3">
        <div>
          <div className="text-xs text-zinc-500">Range</div>
          <div className="font-mono text-sm text-zinc-200">
            {s.range_bottom?.toFixed(2)} – {s.range_top?.toFixed(2)}
          </div>
        </div>
        <ConfidenceBar value={s.confidence} />
        {s.creek_level != null && (
          <div>
            <div className="text-xs text-blue-400">Creek</div>
            <div className="font-mono text-sm text-zinc-200">{s.creek_level.toFixed(2)}</div>
          </div>
        )}
        {s.ice_level != null && (
          <div>
            <div className="text-xs text-red-400">Ice</div>
            <div className="font-mono text-sm text-zinc-200">{s.ice_level.toFixed(2)}</div>
          </div>
        )}
      </div>

      {/* Event timeline */}
      <div>
        <div className="mb-2 text-xs font-bold uppercase text-zinc-500">Event Timeline</div>
        <EventTimeline events={events} />
      </div>

      {/* Status */}
      {!s.is_active && (
        <div className="rounded border border-zinc-700 bg-zinc-900/50 p-2 text-xs">
          {s.completed_at && <span className="text-emerald-400">✅ Completed: {new Date(s.completed_at).toLocaleString()}</span>}
          {s.failed_at && (
            <span className="text-red-400">
              ❌ Failed: {new Date(s.failed_at).toLocaleString()}
              {s.failure_reason && ` — ${s.failure_reason}`}
            </span>
          )}
        </div>
      )}

      {/* Schematic reference — highlight current phase row */}
      <div className="rounded border border-zinc-800 bg-zinc-900/30 p-3">
        <div className="mb-1 text-xs font-bold uppercase text-zinc-500">Schematic Reference</div>
        <SchematicReference schematic={s.schematic} current={s.current_phase} />
      </div>
    </div>
  );
}

function SchematicReference({ schematic, current }: { schematic: string; current: string }) {
  const bullish = schematic === "accumulation" || schematic === "reaccumulation";
  const rows: { phase: string; text: string }[] = bullish
    ? [
        { phase: "A", text: "Phase A: PS → SC → AR → ST" },
        { phase: "B", text: "Phase B: ST-B (range testing, volume declines)" },
        { phase: "C", text: "Phase C: Spring / Shakeout (false break below support)" },
        { phase: "D", text: "Phase D: SOS → LPS → JAC → BUEC" },
        { phase: "E", text: "Phase E: Markup (trend out of range)" },
      ]
    : [
        { phase: "A", text: "Phase A: PS → BC → AR → ST" },
        { phase: "B", text: "Phase B: UA (upthrust action, volume declines)" },
        { phase: "C", text: "Phase C: UTAD (false break above resistance)" },
        { phase: "D", text: "Phase D: SOW → LPSY → Break of Ice" },
        { phase: "E", text: "Phase E: Markdown (trend out of range)" },
      ];
  return (
    <div className="font-mono text-xs leading-relaxed">
      {rows.map((r) => {
        const isCurrent = r.phase === current;
        const cls = isCurrent
          ? "text-zinc-100 font-bold bg-zinc-800/80 rounded px-1"
          : "text-zinc-500";
        return (
          <div key={r.phase} className={cls}>
            {isCurrent ? "▶ " : "  "}
            {r.text}
          </div>
        );
      })}
    </div>
  );
}

// =========================================================================
// Main Page
// =========================================================================

type StatusFilter = "active" | "completed" | "failed" | "all";

const STATUS_TABS: { key: StatusFilter; label: string }[] = [
  { key: "active",    label: "Active" },
  { key: "completed", label: "Completed" },
  { key: "failed",    label: "Failed" },
  { key: "all",       label: "All" },
];

interface PhaseGroup {
  exchange: string;
  symbol: string;
  interval: string;
  current_phase: string;
  total: number;
  active: number;
  completed: number;
  failed: number;
  first_seen: string;
  last_seen: string;
}

const PHASE_ORDER = ["A", "B", "C", "D", "E"] as const;

/// Collapse raw per-phase rows into one row per (exchange, symbol, interval)
/// with a phase-keyed counts map. Drives the timeframe-grouped summary
/// requested by the operator ("ilk kayıttan bu güne kadar olan fazları
/// kendi timeframe içerisinde grupla").
function aggregateByTimeframe(groups: PhaseGroup[]) {
  const map = new Map<
    string,
    {
      exchange: string;
      symbol: string;
      interval: string;
      total: number;
      active: number;
      completed: number;
      failed: number;
      first_seen: string;
      last_seen: string;
      phases: Record<string, number>;
    }
  >();
  for (const g of groups) {
    const key = `${g.exchange}|${g.symbol}|${g.interval}`;
    const prev = map.get(key);
    if (!prev) {
      map.set(key, {
        exchange: g.exchange,
        symbol: g.symbol,
        interval: g.interval,
        total: g.total,
        active: g.active,
        completed: g.completed,
        failed: g.failed,
        first_seen: g.first_seen,
        last_seen: g.last_seen,
        phases: { [g.current_phase]: g.total },
      });
    } else {
      prev.total += g.total;
      prev.active += g.active;
      prev.completed += g.completed;
      prev.failed += g.failed;
      if (g.first_seen < prev.first_seen) prev.first_seen = g.first_seen;
      if (g.last_seen > prev.last_seen) prev.last_seen = g.last_seen;
      prev.phases[g.current_phase] = (prev.phases[g.current_phase] ?? 0) + g.total;
    }
  }
  return Array.from(map.values()).sort((a, b) =>
    a.symbol.localeCompare(b.symbol) || a.interval.localeCompare(b.interval),
  );
}

function PhaseGroupTable({
  rows,
  onPick,
}: {
  rows: ReturnType<typeof aggregateByTimeframe>;
  onPick: (symbol: string, interval: string) => void;
}) {
  if (rows.length === 0) {
    return <div className="text-sm text-zinc-600">No structures recorded yet.</div>;
  }
  return (
    <div className="overflow-x-auto rounded border border-zinc-800">
      <table className="w-full text-xs">
        <thead className="bg-zinc-900/60 text-zinc-400">
          <tr>
            <th className="px-2 py-1.5 text-left">Symbol</th>
            <th className="px-2 py-1.5 text-left">TF</th>
            {PHASE_ORDER.map((p) => (
              <th key={p} className="px-2 py-1.5 text-center">Phase {p}</th>
            ))}
            <th className="px-2 py-1.5 text-center">Active</th>
            <th className="px-2 py-1.5 text-center">Completed</th>
            <th className="px-2 py-1.5 text-center">Failed</th>
            <th className="px-2 py-1.5 text-center">Total</th>
            <th className="px-2 py-1.5 text-left">First</th>
            <th className="px-2 py-1.5 text-left">Last</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr
              key={`${r.exchange}-${r.symbol}-${r.interval}`}
              onClick={() => onPick(r.symbol, r.interval)}
              className="cursor-pointer border-t border-zinc-800 hover:bg-zinc-900/60"
            >
              <td className="px-2 py-1 font-mono font-semibold text-zinc-200">{r.symbol}</td>
              <td className="px-2 py-1 font-mono text-zinc-400">{r.interval}</td>
              {PHASE_ORDER.map((p) => {
                const n = r.phases[p] ?? 0;
                return (
                  <td key={p} className="px-2 py-1 text-center">
                    {n > 0 ? (
                      <span className={`rounded px-1.5 py-0.5 ${PHASE_COLORS[p] ?? ""}`}>{n}</span>
                    ) : (
                      <span className="text-zinc-700">0</span>
                    )}
                  </td>
                );
              })}
              <td className="px-2 py-1 text-center text-emerald-400">{r.active}</td>
              <td className="px-2 py-1 text-center text-blue-400">{r.completed}</td>
              <td className="px-2 py-1 text-center text-red-400">{r.failed}</td>
              <td className="px-2 py-1 text-center font-semibold text-zinc-200">{r.total}</td>
              <td className="px-2 py-1 text-zinc-500">{new Date(r.first_seen).toLocaleDateString()}</td>
              <td className="px-2 py-1 text-zinc-500">{new Date(r.last_seen).toLocaleDateString()}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function Wyckoff() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [status, setStatus] = useState<StatusFilter>("active");

  // Exchange / symbol / timeframe filters — applied to both the
  // list feed and the phase-group summary.
  const [exchange, setExchange] = useState<string>("");
  const [symbol, setSymbol] = useState<string>("");
  const [interval, setIntervalF] = useState<string>("");

  const queryString = (() => {
    const params = new URLSearchParams();
    params.set("limit", "200");
    if (exchange) params.set("exchange", exchange);
    if (symbol) params.set("symbol", symbol);
    if (interval) params.set("interval", interval);
    return params.toString();
  })();

  // Active tab still hits /v2/wyckoff/active but we forward symbol/interval
  // so the two paths converge on filtered results.
  const activeParams = (() => {
    const p = new URLSearchParams();
    if (symbol) p.set("symbol", symbol);
    if (interval) p.set("interval", interval);
    return p.toString();
  })();

  const listQuery = useQuery({
    queryKey: ["v2", "wyckoff", "list", status, exchange, symbol, interval],
    queryFn: () =>
      status === "active"
        ? apiFetch<{ structures: WyckoffStructure[] }>(
            `/v2/wyckoff/active${activeParams ? `?${activeParams}` : ""}`,
          )
        : apiFetch<{ structures: WyckoffStructure[] }>(
            `/v2/wyckoff/recent?status=${status}&${queryString}`,
          ),
    refetchInterval: status === "active" ? 15_000 : 60_000,
  });

  const groupsQuery = useQuery({
    queryKey: ["v2", "wyckoff", "phase-groups", exchange, symbol, interval],
    queryFn: () => {
      const p = new URLSearchParams();
      if (exchange) p.set("exchange", exchange);
      if (symbol) p.set("symbol", symbol);
      if (interval) p.set("interval", interval);
      const qs = p.toString();
      return apiFetch<{ groups: PhaseGroup[] }>(
        `/v2/wyckoff/phase-groups${qs ? `?${qs}` : ""}`,
      );
    },
    refetchInterval: 60_000,
  });

  const structures = listQuery.data?.structures ?? [];
  const aggregated = aggregateByTimeframe(groupsQuery.data?.groups ?? []);

  // Derive option lists from whatever groups the backend returned so we
  // don't need a separate catalog endpoint; as new symbols/intervals
  // enter the table they appear automatically.
  const allGroups = groupsQuery.data?.groups ?? [];
  const exchanges = Array.from(new Set(allGroups.map((g) => g.exchange))).sort();
  const symbols = Array.from(new Set(allGroups.map((g) => g.symbol))).sort();
  const intervals = Array.from(new Set(allGroups.map((g) => g.interval))).sort();

  const filtersActive = exchange || symbol || interval;

  return (
    <div className="mx-auto max-w-7xl space-y-6 p-6">
      <h1 className="text-xl font-bold text-zinc-100">Wyckoff Structures</h1>

      {/* Filters row: exchange / symbol / timeframe. */}
      <div className="flex flex-wrap items-center gap-2 rounded border border-zinc-800 bg-zinc-900/40 p-3">
        <span className="text-xs font-semibold uppercase text-zinc-500">Filters</span>
        <select
          value={exchange}
          onChange={(e) => setExchange(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200"
        >
          <option value="">All Exchanges</option>
          {exchanges.map((x) => <option key={x} value={x}>{x}</option>)}
        </select>
        <select
          value={symbol}
          onChange={(e) => setSymbol(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200"
        >
          <option value="">All Symbols</option>
          {symbols.map((x) => <option key={x} value={x}>{x}</option>)}
        </select>
        <select
          value={interval}
          onChange={(e) => setIntervalF(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200"
        >
          <option value="">All Timeframes</option>
          {intervals.map((x) => <option key={x} value={x}>{x}</option>)}
        </select>
        {filtersActive && (
          <button
            onClick={() => { setExchange(""); setSymbol(""); setIntervalF(""); }}
            className="rounded border border-zinc-700 bg-zinc-800 px-2 py-1 text-xs text-zinc-300 hover:border-zinc-500"
          >
            Clear
          </button>
        )}
      </div>

      {/* Per-timeframe phase distribution. Historical summary: counts
         from the first stored structure for each (symbol, interval). */}
      <div className="space-y-2">
        <h2 className="text-sm font-bold uppercase text-zinc-500">
          Phases by Timeframe
        </h2>
        <PhaseGroupTable
          rows={aggregated}
          onPick={(sym, itv) => { setSymbol(sym); setIntervalF(itv); }}
        />
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        {/* Left: structures list with status tabs */}
        <div className="space-y-3 lg:col-span-1">
          <div className="flex flex-wrap gap-1">
            {STATUS_TABS.map((t) => (
              <button
                key={t.key}
                onClick={() => setStatus(t.key)}
                className={`rounded px-2 py-1 text-xs font-semibold transition ${
                  status === t.key
                    ? "bg-zinc-100 text-zinc-900"
                    : "border border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-500"
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>
          <h2 className="text-sm font-bold uppercase text-zinc-500">
            {STATUS_TABS.find((t) => t.key === status)?.label} ({structures.length})
          </h2>
          {structures.length === 0 && (
            <div className="text-sm text-zinc-600">
              No {status === "all" ? "" : status} Wyckoff structures.
            </div>
          )}
          {structures.map((s) => (
            <StructureCard key={s.id} s={s} onSelect={setSelectedId} />
          ))}
        </div>

        {/* Right: detail panel */}
        <div className="lg:col-span-2">
          {selectedId ? (
            <StructureDetail id={selectedId} />
          ) : (
            <div className="flex h-64 items-center justify-center text-sm text-zinc-600">
              Select a structure to see details
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
