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
        <span className={`text-sm font-semibold ${SCHEMATIC_COLORS[s.schematic] ?? "text-zinc-300"}`}>
          {s.schematic.toUpperCase()}
        </span>
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
        <span className={`text-sm font-semibold ${SCHEMATIC_COLORS[s.schematic] ?? ""}`}>
          {s.schematic.toUpperCase()}
        </span>
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

      {/* Schematic reference */}
      <div className="rounded border border-zinc-800 bg-zinc-900/30 p-3">
        <div className="mb-1 text-xs font-bold uppercase text-zinc-500">Schematic Reference</div>
        <div className="font-mono text-xs text-zinc-400 leading-relaxed whitespace-pre">
          {s.schematic === "accumulation" || s.schematic === "reaccumulation"
            ? `Phase A: PS → SC → AR → ST
Phase B: ST-B (range testing, volume declines)
Phase C: Spring / Shakeout (false break below support)
Phase D: SOS → LPS → JAC → BUEC
Phase E: Markup (trend out of range)`
            : `Phase A: PS → BC → AR → ST
Phase B: UA (upthrust action, volume declines)
Phase C: UTAD (false break above resistance)
Phase D: SOW → LPSY → Break of Ice
Phase E: Markdown (trend out of range)`}
        </div>
      </div>
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

export function Wyckoff() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [status, setStatus] = useState<StatusFilter>("active");

  // Active tab hits /v2/wyckoff/active (no-limit, auto-refresh faster);
  // other tabs hit /v2/wyckoff/recent?status=... with higher cap.
  const listQuery = useQuery({
    queryKey: ["v2", "wyckoff", "list", status],
    queryFn: () =>
      status === "active"
        ? apiFetch<{ structures: WyckoffStructure[] }>("/v2/wyckoff/active")
        : apiFetch<{ structures: WyckoffStructure[] }>(
            `/v2/wyckoff/recent?status=${status}&limit=200`,
          ),
    refetchInterval: status === "active" ? 15_000 : 60_000,
  });

  const structures = listQuery.data?.structures ?? [];

  return (
    <div className="mx-auto max-w-6xl space-y-6 p-6">
      <h1 className="text-xl font-bold text-zinc-100">Wyckoff Structures</h1>

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
