import { useEffect, useState, useCallback } from "react";
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

// ─── Degree colors (higher = warmer) ────────────────────────────────
const DEGREE_COLORS: Record<string, string> = {
  "Grand Supercycle": "#ef4444",
  Supercycle: "#f97316",
  Cycle: "#eab308",
  Primary: "#22c55e",
  Intermediate: "#14b8a6",
  Minor: "#3b82f6",
  Minute: "#8b5cf6",
  Minuette: "#a855f7",
  Subminuette: "#6b7280",
};

const DIR_ICON: Record<string, string> = {
  bullish: "\u25B2",
  bearish: "\u25BC",
};

// ─── Tree Node Component ─────────────────────────────────────────────
function TreeNode({
  node,
  depth,
  expanded,
  toggleExpand,
  selected,
  onSelect,
}: {
  node: WaveNode;
  depth: number;
  expanded: Set<string>;
  toggleExpand: (id: string) => void;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const hasChildren = node.children.length > 0;
  const isOpen = expanded.has(node.id);
  const isSelected = selected === node.id;
  const color = DEGREE_COLORS[node.degree] ?? "#9ca3af";
  const dir = DIR_ICON[node.direction] ?? "";
  const pctChange =
    Number(node.price_start) !== 0
      ? (
          ((Number(node.price_end) - Number(node.price_start)) /
            Number(node.price_start)) *
          100
        ).toFixed(2)
      : "0.00";
  const timeLabel = node.time_start
    ? new Date(node.time_start).toLocaleDateString()
    : "";

  return (
    <div>
      <div
        className={`flex items-center gap-1 py-1 px-2 rounded cursor-pointer transition-colors text-[12px] font-mono ${
          isSelected
            ? "bg-zinc-700/60 ring-1 ring-sky-500/50"
            : "hover:bg-zinc-800/60"
        }`}
        style={{ paddingLeft: `${depth * 20 + 8}px` }}
        onClick={() => onSelect(node.id)}
      >
        {/* expand/collapse toggle */}
        <span
          className="w-4 text-center text-zinc-500 select-none"
          onClick={(e) => {
            e.stopPropagation();
            if (hasChildren) toggleExpand(node.id);
          }}
        >
          {hasChildren ? (isOpen ? "\u25BE" : "\u25B8") : "\u00B7"}
        </span>

        {/* degree badge */}
        <span
          className="inline-block px-1.5 py-0.5 rounded text-[10px] font-bold"
          style={{ backgroundColor: color + "22", color }}
        >
          {node.degree}
        </span>

        {/* wave number */}
        <span className="text-white font-bold">
          {node.wave_number ?? "?"}
        </span>

        {/* direction + kind */}
        <span
          className={`text-[11px] ${
            node.direction === "bullish" ? "text-emerald-400" : "text-red-400"
          }`}
        >
          {dir} {node.kind}
        </span>

        {/* TF */}
        <span className="text-zinc-500 text-[10px]">{node.timeframe}</span>

        {/* price range */}
        <span className="text-zinc-400 text-[10px] ml-auto">
          {Number(node.price_start).toFixed(2)} {"\u2192"}{" "}
          {Number(node.price_end).toFixed(2)}
        </span>

        {/* % change */}
        <span
          className={`text-[10px] w-16 text-right ${
            Number(pctChange) >= 0 ? "text-emerald-400" : "text-red-400"
          }`}
        >
          {Number(pctChange) >= 0 ? "+" : ""}
          {pctChange}%
        </span>

        {/* score */}
        <span className="text-zinc-500 text-[10px] w-10 text-right">
          {(node.structural_score * 100).toFixed(0)}%
        </span>

        {/* time */}
        <span className="text-zinc-600 text-[10px] w-20 text-right">
          {timeLabel}
        </span>
      </div>

      {/* children */}
      {hasChildren && isOpen && (
        <div>
          {node.children.map((child) => (
            <TreeNode
              key={child.id}
              node={child}
              depth={depth + 1}
              expanded={expanded}
              toggleExpand={toggleExpand}
              selected={selected}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Detail Panel ────────────────────────────────────────────────────
function DetailPanel({ node }: { node: WaveNode | null }) {
  if (!node) {
    return (
      <div className="flex items-center justify-center h-full text-zinc-600 text-sm">
        Select a wave to see details
      </div>
    );
  }
  const color = DEGREE_COLORS[node.degree] ?? "#9ca3af";
  return (
    <div className="p-4 space-y-3 text-sm">
      <div className="flex items-center gap-2">
        <span
          className="px-2 py-1 rounded font-bold text-xs"
          style={{ backgroundColor: color + "22", color }}
        >
          {node.degree}
        </span>
        <span className="text-white font-bold text-lg">
          {node.wave_number ?? "?"}
        </span>
        <span
          className={
            node.direction === "bullish" ? "text-emerald-400" : "text-red-400"
          }
        >
          {DIR_ICON[node.direction]} {node.kind}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-zinc-300">
        <div>
          <span className="text-zinc-500">Timeframe:</span> {node.timeframe}
        </div>
        <div>
          <span className="text-zinc-500">State:</span> {node.state}
        </div>
        <div>
          <span className="text-zinc-500">Price Start:</span>{" "}
          {Number(node.price_start).toFixed(2)}
        </div>
        <div>
          <span className="text-zinc-500">Price End:</span>{" "}
          {Number(node.price_end).toFixed(2)}
        </div>
        <div>
          <span className="text-zinc-500">Score:</span>{" "}
          {(node.structural_score * 100).toFixed(1)}%
        </div>
        <div>
          <span className="text-zinc-500">Children:</span>{" "}
          {node.children.length}
        </div>
        {node.time_start && (
          <div>
            <span className="text-zinc-500">Start:</span>{" "}
            {new Date(node.time_start).toLocaleString()}
          </div>
        )}
        {node.time_end && (
          <div>
            <span className="text-zinc-500">End:</span>{" "}
            {new Date(node.time_end).toLocaleString()}
          </div>
        )}
      </div>

      {/* Ancestor breadcrumb */}
      {node.parent_id && (
        <div className="mt-2 pt-2 border-t border-zinc-800">
          <span className="text-zinc-500 text-xs">Parent ID:</span>{" "}
          <span className="text-zinc-400 text-xs font-mono">
            {node.parent_id}
          </span>
        </div>
      )}

      {/* Child summary */}
      {node.children.length > 0 && (
        <div className="mt-2 pt-2 border-t border-zinc-800">
          <div className="text-zinc-500 text-xs mb-1">Sub-waves:</div>
          <div className="flex flex-wrap gap-1">
            {node.children.map((c) => (
              <span
                key={c.id}
                className="px-1.5 py-0.5 rounded text-[10px]"
                style={{
                  backgroundColor:
                    (DEGREE_COLORS[c.degree] ?? "#6b7280") + "22",
                  color: DEGREE_COLORS[c.degree] ?? "#6b7280",
                }}
              >
                {c.wave_number ?? "?"} {c.degree} ({c.timeframe})
              </span>
            ))}
          </div>
        </div>
      )}
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
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);

  const fetchTree = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await apiFetch<WaveTreeResponse>(
        `/v2/wave-tree/${venue}/${symbol}?limit=500`
      );
      setData(res);
      // Auto-expand root nodes
      const rootIds = new Set(res.roots.map((r) => r.id));
      setExpanded(rootIds);
    } catch (e: any) {
      setError(e?.message ?? "Failed to fetch wave tree");
    } finally {
      setLoading(false);
    }
  }, [venue, symbol]);

  useEffect(() => {
    fetchTree();
  }, [fetchTree]);

  const toggleExpand = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // Find selected node recursively
  const findNode = (nodes: WaveNode[], id: string): WaveNode | null => {
    for (const n of nodes) {
      if (n.id === id) return n;
      const found = findNode(n.children, id);
      if (found) return found;
    }
    return null;
  };

  const selectedNode = data && selected ? findNode(data.roots, selected) : null;

  // Expand all / collapse all
  const collectAllIds = (nodes: WaveNode[]): string[] => {
    const ids: string[] = [];
    for (const n of nodes) {
      ids.push(n.id);
      ids.push(...collectAllIds(n.children));
    }
    return ids;
  };

  return (
    <div className="flex flex-col h-full">
      {/* Top bar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-zinc-800 bg-zinc-950">
        <span className="text-sm font-semibold text-zinc-300">
          Elliott Deep — Wave Tree
        </span>
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
          className="px-3 py-1 rounded bg-sky-600 hover:bg-sky-500 text-xs text-white"
        >
          Load
        </button>
        {data && (
          <>
            <span className="text-[10px] text-zinc-500">
              {data.total_waves} waves
            </span>
            <button
              onClick={() =>
                setExpanded(new Set(collectAllIds(data.roots)))
              }
              className="text-[10px] text-zinc-500 hover:text-zinc-300"
            >
              Expand All
            </button>
            <button
              onClick={() => setExpanded(new Set())}
              className="text-[10px] text-zinc-500 hover:text-zinc-300"
            >
              Collapse All
            </button>
          </>
        )}
      </div>

      {/* Content */}
      <div className="flex flex-1 overflow-hidden">
        {/* Tree panel */}
        <div className="flex-1 overflow-auto border-r border-zinc-800">
          {loading && (
            <div className="p-4 text-zinc-500 text-sm">Loading...</div>
          )}
          {error && (
            <div className="p-4 text-red-400 text-sm">{error}</div>
          )}
          {data && data.roots.length === 0 && (
            <div className="p-4 text-zinc-600 text-sm">
              No wave segments found for {data.symbol}. Elliott detections
              will populate this tree automatically.
            </div>
          )}
          {data &&
            data.roots.map((root) => (
              <TreeNode
                key={root.id}
                node={root}
                depth={0}
                expanded={expanded}
                toggleExpand={toggleExpand}
                selected={selected}
                onSelect={setSelected}
              />
            ))}
        </div>

        {/* Detail panel */}
        <div className="w-80 bg-zinc-900/50 overflow-auto">
          <DetailPanel node={selectedNode} />
        </div>
      </div>

      {/* Legend */}
      <div className="flex items-center gap-3 px-4 py-1 border-t border-zinc-800 bg-zinc-950">
        {Object.entries(DEGREE_COLORS).map(([deg, col]) => (
          <span
            key={deg}
            className="text-[9px] px-1 rounded"
            style={{ color: col, backgroundColor: col + "15" }}
          >
            {deg}
          </span>
        ))}
      </div>
    </div>
  );
}

export default WaveTree;
