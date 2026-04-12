import { useEffect, useState, useCallback, Fragment } from "react";
import { apiFetch } from "../lib/api";

// ─── Types ───────────────────────────────────────────────────────────
interface WaveNode {
  id: string;
  parent_id: string | null;
  timeframe: string;
  degree: string;
  kind: string;
  direction: string;
  wave_number: string | null;
  price_start: string;
  price_end: string;
  time_start: string | null;
  time_end: string | null;
  structural_score: number;
  state: string;
  children: WaveNode[];
}

interface WaveTreeResponse {
  exchange: string;
  symbol: string;
  roots: WaveNode[];
  total_waves: number;
}

// ─── TF hierarchy (top-down) ────────────────────────────────────────
const TF_ORDER = ["1mo", "1w", "1d", "4h", "1h", "15m", "5m", "1m"];
const TF_LABELS: Record<string, string> = {
  "1mo": "1M", "1w": "1W", "1d": "1D", "4h": "4H",
  "1h": "1H", "15m": "15m", "5m": "5m", "1m": "1m",
};
const DEGREE_FOR_TF: Record<string, string> = {
  "1mo": "Supercycle", "1w": "Cycle", "1d": "Primary", "4h": "Intermediate",
  "1h": "Minor", "15m": "Minute", "5m": "Minuette", "1m": "Subminuette",
};

// ─── Colors ─────────────────────────────────────────────────────────
const DEGREE_COLORS: Record<string, string> = {
  Supercycle: "#f97316", Cycle: "#eab308", Primary: "#22c55e",
  Intermediate: "#14b8a6", Minor: "#3b82f6", Minute: "#8b5cf6",
  Minuette: "#a855f7", Subminuette: "#6b7280",
};

// Formation color by state + kind
function formationColor(state: string, kind: string): string {
  if (state === "completed" || state === "invalidated") return "#52525b"; // gray
  if (kind === "impulse") return "#22c55e"; // green
  return "#ef4444"; // red for corrective
}
function formationBg(state: string, kind: string): string {
  if (state === "completed" || state === "invalidated") return "#27272a";
  if (kind === "impulse") return "#052e16";
  return "#450a0a";
}

// ─── Helpers ────────────────────────────────────────────────────────
function fmtPrice(p: string) {
  const n = Number(p);
  return n >= 1000 ? n.toLocaleString("en", { maximumFractionDigits: 2 })
    : n >= 1 ? n.toFixed(4) : n.toFixed(6);
}
function fmtDate(d: string | null) {
  if (!d) return "—";
  return new Date(d).toLocaleDateString("en", { year: "2-digit", month: "short", day: "numeric" });
}
function fmtPct(start: string, end: string) {
  const s = Number(start);
  if (s === 0) return "0%";
  const pct = ((Number(end) - s) / s) * 100;
  const sign = pct >= 0 ? "+" : "";
  return `${sign}${pct.toFixed(2)}%`;
}

// ─── Group waves by formation (same parent or same detection) ───────
// At each TF level we show "formations" (groups of consecutive wave segments)
// For now each root at a TF is one formation; children are its wave segments.
interface Formation {
  id: string; // first wave's id
  kind: string;
  direction: string;
  degree: string;
  state: string;
  timeframe: string;
  waves: WaveNode[];
  priceStart: string;
  priceEnd: string;
  timeStart: string | null;
  timeEnd: string | null;
  score: number;
  children: WaveNode[]; // sub-formations at lower TF (collected from all wave children)
}

function collectFormations(nodes: WaveNode[], tf: string): Formation[] {
  // Filter nodes matching this TF
  const atTf = nodes.filter((n) => n.timeframe === tf);
  if (atTf.length === 0) return [];

  // Each node at this level represents one wave segment of a formation.
  // Group by: same parent_id → same formation. Orphans (no parent) are individual formations.
  const groups: Record<string, WaveNode[]> = {};
  for (const n of atTf) {
    const key = n.parent_id ?? `solo_${n.id}`;
    if (!groups[key]) groups[key] = [];
    groups[key].push(n);
  }

  return Object.values(groups).map((waves) => {
    // Sort by time
    waves.sort((a, b) => (a.time_start ?? "").localeCompare(b.time_start ?? ""));
    const first = waves[0];
    const last = waves[waves.length - 1];
    const allChildren = waves.flatMap((w) => w.children);
    const anyActive = waves.some((w) => w.state === "active");
    return {
      id: first.id,
      kind: first.kind,
      direction: first.direction,
      degree: first.degree,
      state: anyActive ? "active" : "completed",
      timeframe: tf,
      waves,
      priceStart: first.price_start,
      priceEnd: last.price_end,
      timeStart: first.time_start,
      timeEnd: last.time_end,
      score: waves.reduce((sum, w) => sum + w.structural_score, 0) / waves.length,
      children: allChildren,
    };
  });
}

// ─── Wave Segment Row ───────────────────────────────────────────────
function WaveSegment({ w, isLast }: { w: WaveNode; isLast: boolean }) {
  const col = formationColor(w.state, w.kind);
  const isCompleted = w.state === "completed" || w.state === "invalidated";
  return (
    <div className={`flex items-center gap-2 py-1 px-3 text-[11px] font-mono border-l-2 ${isLast ? "" : "border-b border-zinc-800/30"}`}
      style={{ borderLeftColor: col + "60" }}>
      <span className="w-10 font-bold" style={{ color: isCompleted ? "#71717a" : col }}>
        {w.wave_number ?? "?"}
      </span>
      <span className={`text-[10px] ${w.direction === "bullish" ? "text-emerald-400/70" : "text-red-400/70"}`}>
        {w.direction === "bullish" ? "\u25B2" : "\u25BC"}
      </span>
      <span className={isCompleted ? "text-zinc-500" : "text-zinc-300"}>
        {fmtPrice(w.price_start)} → {fmtPrice(w.price_end)}
      </span>
      <span className={`text-[10px] ${Number(w.price_end) >= Number(w.price_start) ? "text-emerald-400/60" : "text-red-400/60"}`}>
        {fmtPct(w.price_start, w.price_end)}
      </span>
      <span className="text-zinc-600 text-[10px] ml-auto">
        {fmtDate(w.time_start)} — {fmtDate(w.time_end)}
      </span>
      <span className="text-zinc-600 text-[10px] w-10 text-right">
        {(w.structural_score * 100).toFixed(0)}%
      </span>
    </div>
  );
}

// ─── Formation Card ─────────────────────────────────────────────────
function FormationCard({
  f,
  onDrillDown,
}: {
  f: Formation;
  onDrillDown: (children: WaveNode[]) => void;
}) {
  const [expanded, setExpanded] = useState(f.state === "active");
  const col = formationColor(f.state, f.kind);
  const bg = formationBg(f.state, f.kind);
  const isCompleted = f.state === "completed" || f.state === "invalidated";
  const hasLowerTf = f.children.length > 0;

  return (
    <div className="rounded-lg overflow-hidden mb-2" style={{ backgroundColor: bg, border: `1px solid ${col}30` }}>
      {/* Formation header */}
      <div
        className="flex items-center gap-2 px-3 py-2 cursor-pointer select-none"
        onClick={() => setExpanded(!expanded)}
      >
        <span className="text-[10px] text-zinc-500">{expanded ? "\u25BE" : "\u25B8"}</span>
        <span className="font-bold text-sm" style={{ color: isCompleted ? "#71717a" : col }}>
          {f.kind === "impulse" ? "Impulse" : "Corrective"}
        </span>
        <span className={`text-xs ${f.direction === "bullish" ? "text-emerald-400" : "text-red-400"}`}>
          {f.direction === "bullish" ? "\u25B2" : "\u25BC"} {f.direction}
        </span>
        <span className="px-1.5 py-0.5 rounded text-[9px] font-bold"
          style={{ backgroundColor: (DEGREE_COLORS[f.degree] ?? "#6b7280") + "25", color: DEGREE_COLORS[f.degree] ?? "#6b7280" }}>
          {f.degree}
        </span>
        {isCompleted && (
          <span className="text-[9px] px-1.5 py-0.5 rounded bg-zinc-700/50 text-zinc-500">COMPLETED</span>
        )}
        <span className="ml-auto text-[10px] text-zinc-400">
          {fmtPrice(f.priceStart)} → {fmtPrice(f.priceEnd)}
        </span>
        <span className={`text-[10px] ${Number(f.priceEnd) >= Number(f.priceStart) ? "text-emerald-400/70" : "text-red-400/70"}`}>
          {fmtPct(f.priceStart, f.priceEnd)}
        </span>
        <span className="text-zinc-600 text-[10px]">{(f.score * 100).toFixed(0)}%</span>
      </div>

      {/* Wave segments */}
      {expanded && (
        <div className="border-t" style={{ borderColor: col + "20" }}>
          {f.waves.map((w, i) => (
            <WaveSegment key={w.id} w={w} isLast={i === f.waves.length - 1} />
          ))}
          {/* Drill-down button */}
          {hasLowerTf && (
            <div className="px-3 py-1.5 border-t" style={{ borderColor: col + "15" }}>
              <button
                onClick={(e) => { e.stopPropagation(); onDrillDown(f.children); }}
                className="text-[10px] text-sky-400 hover:text-sky-300 flex items-center gap-1"
              >
                <span>▾</span> Alt periyoda in ({f.children.length} alt dalga)
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── TF Level Panel ─────────────────────────────────────────────────
function TfLevel({
  tf,
  nodes,
  onDrillDown,
}: {
  tf: string;
  nodes: WaveNode[];
  onDrillDown: (tf: string, children: WaveNode[]) => void;
}) {
  const formations = collectFormations(nodes, tf);
  const degree = DEGREE_FOR_TF[tf] ?? tf;
  const degColor = DEGREE_COLORS[degree] ?? "#6b7280";

  if (formations.length === 0) {
    return (
      <div className="mb-1 px-4 py-2 rounded-lg bg-zinc-900/50 border border-zinc-800/50">
        <div className="flex items-center gap-2">
          <span className="font-bold text-lg" style={{ color: degColor }}>{TF_LABELS[tf] ?? tf}</span>
          <span className="text-[10px] px-1.5 py-0.5 rounded" style={{ backgroundColor: degColor + "15", color: degColor }}>
            {degree}
          </span>
          <span className="text-zinc-600 text-xs ml-2">Formasyon bulunamadi</span>
        </div>
      </div>
    );
  }

  const activeCount = formations.filter((f) => f.state === "active").length;
  const completedCount = formations.filter((f) => f.state !== "active").length;

  return (
    <div className="mb-1">
      {/* TF header */}
      <div className="flex items-center gap-2 px-4 py-2 rounded-t-lg bg-zinc-900/80 border border-zinc-800/50 border-b-0">
        <span className="font-bold text-lg" style={{ color: degColor }}>{TF_LABELS[tf] ?? tf}</span>
        <span className="text-[10px] px-1.5 py-0.5 rounded font-bold" style={{ backgroundColor: degColor + "20", color: degColor }}>
          {degree}
        </span>
        <span className="text-[10px] text-zinc-500 ml-2">
          {activeCount > 0 && <span className="text-emerald-400">{activeCount} aktif</span>}
          {activeCount > 0 && completedCount > 0 && " · "}
          {completedCount > 0 && <span className="text-zinc-500">{completedCount} tamamlandi</span>}
        </span>
      </div>
      {/* Formation cards */}
      <div className="px-4 py-2 rounded-b-lg bg-zinc-900/40 border border-zinc-800/50 border-t-0">
        {formations.map((f) => (
          <FormationCard
            key={f.id}
            f={f}
            onDrillDown={(children) => onDrillDown(tf, children)}
          />
        ))}
      </div>
    </div>
  );
}

// ─── Breadcrumb ─────────────────────────────────────────────────────
function Breadcrumb({
  path,
  onNavigate,
}: {
  path: { tf: string; label: string }[];
  onNavigate: (index: number) => void;
}) {
  return (
    <div className="flex items-center gap-1 text-[11px]">
      {path.map((item, i) => (
        <Fragment key={i}>
          {i > 0 && <span className="text-zinc-600">›</span>}
          <button
            onClick={() => onNavigate(i)}
            className={`px-1.5 py-0.5 rounded ${
              i === path.length - 1
                ? "bg-sky-900/30 text-sky-400"
                : "text-zinc-500 hover:text-zinc-300"
            }`}
          >
            {item.label}
          </button>
        </Fragment>
      ))}
    </div>
  );
}

// ─── Main Page ───────────────────────────────────────────────────────
export function WaveTree() {
  const [venue, setVenue] = useState("binance");
  const [symbol, setSymbol] = useState("BTCUSDT");
  const [data, setData] = useState<WaveTreeResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Navigation stack: each entry is { tf, nodes, label }
  // Start at root level showing all TFs, drill-down narrows to specific children
  const [navStack, setNavStack] = useState<{ tf: string; nodes: WaveNode[]; label: string }[]>([]);

  const fetchTree = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await apiFetch<WaveTreeResponse>(
        `/v2/wave-tree/${venue}/${symbol}?limit=1000`
      );
      setData(res);
      setNavStack([]); // reset navigation
    } catch (e: any) {
      setError(e?.message ?? "Failed to fetch wave tree");
    } finally {
      setLoading(false);
    }
  }, [venue, symbol]);

  useEffect(() => {
    fetchTree();
  }, [fetchTree]);

  // Flatten tree into all nodes recursively
  const flattenAll = (nodes: WaveNode[]): WaveNode[] => {
    const result: WaveNode[] = [];
    for (const n of nodes) {
      result.push(n);
      result.push(...flattenAll(n.children));
    }
    return result;
  };

  // Current view: either root (all TFs) or drilled-down (specific children at lower TF)
  const currentNodes = navStack.length > 0
    ? navStack[navStack.length - 1].nodes
    : data?.roots ?? [];

  // Determine which TFs to show at current level
  const currentTfIndex = navStack.length > 0
    ? TF_ORDER.indexOf(navStack[navStack.length - 1].tf) + 1
    : 0;

  // For root view, show all TFs. For drilled-down, show next TF down.
  const allFlat = flattenAll(currentNodes);
  const tfsToShow = navStack.length === 0
    ? TF_ORDER.filter((tf) => allFlat.some((n) => n.timeframe === tf))
    : [TF_ORDER[currentTfIndex]].filter(Boolean).filter((tf) => allFlat.some((n) => n.timeframe === tf));

  // If drilled down but no data at next TF, show what we have
  const effectiveTfs = tfsToShow.length > 0
    ? tfsToShow
    : [...new Set(allFlat.map((n) => n.timeframe))].sort(
        (a, b) => TF_ORDER.indexOf(a) - TF_ORDER.indexOf(b)
      );

  // Drill-down handler
  const handleDrillDown = (fromTf: string, children: WaveNode[]) => {
    if (children.length === 0) return;
    const degree = DEGREE_FOR_TF[fromTf] ?? fromTf;
    setNavStack((prev) => [
      ...prev,
      { tf: fromTf, nodes: children, label: `${TF_LABELS[fromTf] ?? fromTf} ${degree}` },
    ]);
  };

  // Breadcrumb navigation
  const breadcrumbPath = [
    { tf: "all", label: `${symbol} — Tum TF` },
    ...navStack.map((n) => ({ tf: n.tf, label: n.label })),
  ];

  const handleBreadcrumbNav = (index: number) => {
    if (index === 0) {
      setNavStack([]);
    } else {
      setNavStack((prev) => prev.slice(0, index));
    }
  };

  return (
    <div className="flex flex-col h-full bg-zinc-950">
      {/* Top bar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-zinc-800 bg-zinc-900/80">
        <span className="text-sm font-semibold text-zinc-300">
          Elliott Deep
        </span>
        <div className="h-4 w-px bg-zinc-700" />
        <input
          className="bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 w-24"
          value={venue}
          onChange={(e) => setVenue(e.target.value)}
          placeholder="Venue"
        />
        <input
          className="bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 w-28"
          value={symbol}
          onChange={(e) => setSymbol(e.target.value.toUpperCase())}
          placeholder="Symbol"
        />
        <button
          onClick={fetchTree}
          className="px-3 py-1 rounded bg-sky-600 hover:bg-sky-500 text-xs text-white font-medium"
        >
          Yukle
        </button>
        {data && (
          <span className="text-[10px] text-zinc-500 ml-2">
            {data.total_waves} dalga segmenti
          </span>
        )}
      </div>

      {/* Breadcrumb */}
      <div className="px-4 py-1.5 border-b border-zinc-800/50 bg-zinc-900/40">
        <Breadcrumb path={breadcrumbPath} onNavigate={handleBreadcrumbNav} />
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto px-4 py-3 space-y-1">
        {loading && (
          <div className="flex items-center justify-center h-32 text-zinc-500 text-sm">
            Yukleniyor...
          </div>
        )}
        {error && (
          <div className="p-4 text-red-400 text-sm bg-red-950/20 rounded-lg border border-red-900/30">
            {error}
          </div>
        )}
        {data && effectiveTfs.length === 0 && !loading && (
          <div className="flex flex-col items-center justify-center h-48 text-zinc-600">
            <div className="text-4xl mb-3">🌊</div>
            <div className="text-sm">
              {symbol} icin dalga segmenti bulunamadi.
            </div>
            <div className="text-xs text-zinc-700 mt-1">
              Elliott detektorleri calismaya basladiginda burada goruntulenir.
            </div>
          </div>
        )}
        {data && effectiveTfs.map((tf) => (
          <TfLevel
            key={tf}
            tf={tf}
            nodes={allFlat}
            onDrillDown={handleDrillDown}
          />
        ))}
      </div>

      {/* Legend footer */}
      <div className="flex items-center gap-4 px-4 py-1.5 border-t border-zinc-800 bg-zinc-900/60 text-[9px]">
        <span className="text-zinc-500">Renk kodu:</span>
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-emerald-500" /> Aktif Impulse
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-red-500" /> Aktif Corrective
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-zinc-500" /> Completed
        </span>
        <div className="ml-auto flex items-center gap-2">
          {Object.entries(DEGREE_COLORS).map(([deg, col]) => (
            <span key={deg} style={{ color: col }}>{deg}</span>
          ))}
        </div>
      </div>
    </div>
  );
}

export default WaveTree;
