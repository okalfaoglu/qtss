import { useEffect, useState } from "react";
import { apiFetch } from "../lib/api";

// ─── Types ───────────────────────────────────────────────────────────
interface WaveSegment {
  id: string;
  wave_number: string | null;
  direction: string;
  price_start: string;
  price_end: string;
  time_start: string | null;
  time_end: string | null;
  structural_score: number;
  state: string;
  child_count: number;
}

interface Formation {
  id: string;
  kind: string;
  subkind: string;
  direction: string;
  degree: string;
  state: string;
  price_start: string;
  price_end: string;
  time_start: string | null;
  time_end: string | null;
  avg_score: number;
  waves: WaveSegment[];
}

interface TfLevelResponse {
  exchange: string;
  symbol: string;
  timeframe: string;
  formations: Formation[];
}

// ─── Constants ──────────────────────────────────────────────────────
const TF_CHAIN = ["1mo", "1w", "1d", "4h", "1h", "15m", "5m", "1m"];
const TF_LABEL: Record<string, string> = {
  "1mo": "1M", "1w": "1W", "1d": "1D", "4h": "4H",
  "1h": "1H", "15m": "15m", "5m": "5m", "1m": "1m",
};
const DEGREE_LABEL: Record<string, string> = {
  "1mo": "Supercycle", "1w": "Cycle", "1d": "Primary", "4h": "Intermediate",
  "1h": "Minor", "15m": "Minute", "5m": "Minuette", "1m": "Subminuette",
};
const DEGREE_COLOR: Record<string, string> = {
  Supercycle: "#f97316", Cycle: "#eab308", Primary: "#22c55e",
  Intermediate: "#14b8a6", Minor: "#3b82f6", Minute: "#8b5cf6",
  Minuette: "#a855f7", Subminuette: "#6b7280",
};

// ─── Helpers ────────────────────────────────────────────────────────
function fmtP(p: string) {
  const n = Number(p);
  if (n >= 100) return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
  if (n >= 1) return n.toFixed(4);
  return n.toFixed(6);
}
function fmtD(d: string | null) {
  if (!d) return "—";
  return new Date(d).toLocaleDateString("tr-TR", { day: "2-digit", month: "short", year: "2-digit" });
}
function pctStr(s: string, e: string) {
  const sv = Number(s);
  if (!sv) return "";
  const p = ((Number(e) - sv) / sv) * 100;
  return `${p >= 0 ? "+" : ""}${p.toFixed(2)}%`;
}

// Colors based on state + kind
function stateColor(state: string, kind: string) {
  if (state !== "active") return { text: "#71717a", bg: "#18181b", border: "#27272a" };
  if (kind === "impulse") return { text: "#22c55e", bg: "#052e16", border: "#166534" };
  return { text: "#ef4444", bg: "#450a0a", border: "#991b1b" };
}

// ─── Inline sub-tree for a single wave segment's children ───────────
function WaveChildren({
  venue,
  symbol,
  waveId,
  waveTf,
  depth,
}: {
  venue: string;
  symbol: string;
  waveId: string;
  waveTf: string;
  depth: number;
}) {
  const [children, setChildren] = useState<WaveSegment[] | null>(null);
  const [subFormations, setSubFormations] = useState<TfLevelResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);

  const childTfIdx = TF_CHAIN.indexOf(waveTf) + 1;
  const childTf = TF_CHAIN[childTfIdx] ?? null;

  useEffect(() => {
    if (!childTf) { setLoading(false); return; }
    (async () => {
      try {
        // First try: get direct children of this wave
        const kids = await apiFetch<WaveSegment[]>(
          `/v2/wave-tree/${venue}/${symbol}/${waveId}/children`
        );
        if (kids.length > 0) {
          setChildren(kids);
        } else {
          // Fallback: query by TF
          const res = await apiFetch<TfLevelResponse>(
            `/v2/wave-tree/${venue}/${symbol}/tf/${childTf}`
          );
          if (res.formations.length > 0) setSubFormations(res);
        }
      } catch (e: any) {
        setErr(e?.message ?? "Hata");
      } finally {
        setLoading(false);
      }
    })();
  }, [venue, symbol, childTf, waveId]);

  // Render direct children as wave rows
  const data = subFormations;

  if (loading) return <div className="py-1 text-zinc-600 text-[10px]" style={{ paddingLeft: depth * 32 }}>Yukleniyor...</div>;
  if (err) return <div className="py-1 text-red-400/60 text-[10px]" style={{ paddingLeft: depth * 32 }}>{err}</div>;

  // Direct children of this wave (same formation, lower TF segments)
  if (children && children.length > 0 && childTf) {
    return (
      <>
        {children.map((w) => (
          <WaveRow key={w.id} w={w} tf={childTf} venue={venue} symbol={symbol} depth={depth} />
        ))}
      </>
    );
  }

  // Sub-formations at child TF
  if (data && data.formations.length > 0 && childTf) {
    return (
      <>
        {data.formations.map((f) => (
          <FormationBlock key={f.id} f={f} tf={childTf} venue={venue} symbol={symbol} depth={depth} />
        ))}
      </>
    );
  }

  return (
    <div className="py-1 text-zinc-600 text-[10px]" style={{ paddingLeft: depth * 32 }}>
      {childTf ? `${TF_LABEL[childTf]} — Formasyon bulunamadi` : "En alt seviye"}
    </div>
  );
}

// ─── Single wave segment row (expandable) ───────────────────────────
function WaveRow({
  w,
  tf,
  venue,
  symbol,
  depth,
}: {
  w: WaveSegment;
  tf: string;
  venue: string;
  symbol: string;
  depth: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const isCompleted = w.state !== "active";
  const col = isCompleted ? "#71717a" : w.direction === "bullish" ? "#22c55e" : "#ef4444";
  const hasChildren = w.child_count > 0;
  const pLeft = depth * 32;

  return (
    <>
      <div
        className={`flex items-center gap-2 py-1.5 text-[11px] font-mono border-b border-zinc-800/30 ${
          hasChildren ? "cursor-pointer hover:bg-zinc-800/30" : ""
        }`}
        style={{ paddingLeft: pLeft, paddingRight: 12 }}
        onClick={() => hasChildren && setExpanded(!expanded)}
      >
        {/* Expand indicator */}
        <span className="w-4 text-center text-zinc-600 text-[10px] select-none">
          {hasChildren ? (expanded ? "▾" : "▸") : "·"}
        </span>

        {/* Wave number */}
        <span className="w-12 font-bold" style={{ color: col }}>
          {w.wave_number ?? "?"}
        </span>

        {/* Direction arrow */}
        <span className={`text-[10px] ${w.direction === "bullish" ? "text-emerald-400/60" : "text-red-400/60"}`}>
          {w.direction === "bullish" ? "▲" : "▼"}
        </span>

        {/* Price range */}
        <span className={isCompleted ? "text-zinc-600" : "text-zinc-300"}>
          {fmtP(w.price_start)} → {fmtP(w.price_end)}
        </span>

        {/* % change */}
        <span className={`text-[10px] ${Number(w.price_end) >= Number(w.price_start) ? "text-emerald-400/50" : "text-red-400/50"}`}>
          {pctStr(w.price_start, w.price_end)}
        </span>

        {/* Spacer */}
        <span className="flex-1" />

        {/* Date range */}
        <span className="text-zinc-600 text-[10px]">
          {fmtD(w.time_start)} — {fmtD(w.time_end)}
        </span>

        {/* Score */}
        <span className="text-zinc-600 text-[10px] w-10 text-right">
          {(w.structural_score * 100).toFixed(0)}%
        </span>

        {/* Child count badge */}
        {hasChildren && (
          <span className="text-[9px] px-1 rounded bg-sky-900/30 text-sky-400/60">
            {w.child_count}
          </span>
        )}
      </div>

      {/* Expanded: show children TF level */}
      {expanded && (
        <WaveChildren
          venue={venue}
          symbol={symbol}
          waveId={w.id}
          waveTf={tf}
          depth={depth + 1}
        />
      )}
    </>
  );
}

// ─── Formation block (TF header + kind + wave rows) ─────────────────
function FormationBlock({
  f,
  tf,
  venue,
  symbol,
  depth,
}: {
  f: Formation;
  tf: string;
  venue: string;
  symbol: string;
  depth: number;
}) {
  const [expanded, setExpanded] = useState(f.state === "active");
  const sc = stateColor(f.state, f.kind);
  const degreeColor = DEGREE_COLOR[f.degree] ?? "#6b7280";
  const pLeft = depth * 32;

  return (
    <div className="mb-1">
      {/* Formation header */}
      <div
        className="flex items-center gap-2 py-2 cursor-pointer select-none rounded-t"
        style={{ paddingLeft: pLeft, paddingRight: 12, backgroundColor: sc.bg, borderLeft: `3px solid ${sc.border}` }}
        onClick={() => setExpanded(!expanded)}
      >
        <span className="text-zinc-500 text-[10px]">{expanded ? "▾" : "▸"}</span>

        {/* TF badge */}
        <span className="text-sm font-bold" style={{ color: degreeColor }}>
          {TF_LABEL[tf] ?? tf}
        </span>

        {/* Degree badge */}
        <span className="text-[9px] px-1.5 py-0.5 rounded font-bold"
          style={{ backgroundColor: degreeColor + "20", color: degreeColor }}>
          {f.degree}
        </span>

        {/* Kind + subkind name */}
        <span className="font-semibold text-xs" style={{ color: sc.text }}>
          {f.kind === "impulse" ? "IMPULSE" : "CORRECTIVE"}
        </span>
        {f.subkind && (
          <span className="text-[10px] text-zinc-400 italic">
            {f.subkind.replace(/_/g, " ")}
          </span>
        )}

        {/* Direction */}
        <span className={`text-[10px] ${f.direction === "bullish" ? "text-emerald-400/70" : "text-red-400/70"}`}>
          {f.direction === "bullish" ? "▲" : "▼"}
        </span>

        {/* State badge */}
        {f.state !== "active" && (
          <span className="text-[9px] px-1.5 py-0.5 rounded bg-zinc-700/40 text-zinc-500">
            COMPLETED
          </span>
        )}

        <span className="flex-1" />

        {/* Price range */}
        <span className="text-zinc-400 text-[10px]">
          {fmtP(f.price_start)} → {fmtP(f.price_end)}
        </span>
        <span className={`text-[10px] ${Number(f.price_end) >= Number(f.price_start) ? "text-emerald-400/60" : "text-red-400/60"}`}>
          {pctStr(f.price_start, f.price_end)}
        </span>

        {/* Score */}
        <span className="text-zinc-500 text-[10px]">
          {(f.avg_score * 100).toFixed(0)}%
        </span>

        {/* Wave count */}
        <span className="text-zinc-600 text-[10px]">
          {f.waves.length} dalga
        </span>
      </div>

      {/* Wave segments */}
      {expanded && (
        <div style={{ borderLeft: `3px solid ${sc.border}40` }}>
          {f.waves.map((w) => (
            <WaveRow key={w.id} w={w} tf={tf} venue={venue} symbol={symbol} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

// ─── TF Scanner: walks down TF_CHAIN, shows first found or "yok" ────
function TfScanner({ venue, symbol, activeOnly }: { venue: string; symbol: string; activeOnly: boolean }) {
  const [levels, setLevels] = useState<Record<string, TfLevelResponse | "loading" | "empty" | "error">>({});

  // Scan all TFs on mount (or when activeOnly changes)
  useEffect(() => {
    setLevels({});
    let cancelled = false;
    const qs = activeOnly ? "?active_only=true" : "";
    (async () => {
      for (const tf of TF_CHAIN) {
        if (cancelled) break;
        setLevels((prev) => ({ ...prev, [tf]: "loading" }));
        try {
          const res = await apiFetch<TfLevelResponse>(`/v2/wave-tree/${venue}/${symbol}/tf/${tf}${qs}`);
          if (cancelled) break;
          setLevels((prev) => ({ ...prev, [tf]: res.formations.length > 0 ? res : "empty" }));
        } catch {
          if (cancelled) break;
          setLevels((prev) => ({ ...prev, [tf]: "error" }));
        }
      }
    })();
    return () => { cancelled = true; };
  }, [venue, symbol, activeOnly]);

  return (
    <div className="space-y-0">
      {TF_CHAIN.map((tf) => {
        const level = levels[tf];
        const degreeColor = DEGREE_COLOR[DEGREE_LABEL[tf] ?? ""] ?? "#6b7280";

        // Loading
        if (level === "loading" || level === undefined) {
          return (
            <div key={tf} className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/30">
              <span className="font-bold text-sm" style={{ color: degreeColor }}>{TF_LABEL[tf]}</span>
              <span className="text-[10px] px-1.5 py-0.5 rounded" style={{ backgroundColor: degreeColor + "15", color: degreeColor }}>
                {DEGREE_LABEL[tf]}
              </span>
              <span className="text-zinc-600 text-[10px] animate-pulse">Taraniyor...</span>
            </div>
          );
        }

        // Empty
        if (level === "empty") {
          return (
            <div key={tf} className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/30 opacity-40">
              <span className="font-bold text-sm" style={{ color: degreeColor }}>{TF_LABEL[tf]}</span>
              <span className="text-[10px] px-1.5 py-0.5 rounded" style={{ backgroundColor: degreeColor + "15", color: degreeColor }}>
                {DEGREE_LABEL[tf]}
              </span>
              <span className="text-zinc-600 text-[10px]">Formasyon yok</span>
            </div>
          );
        }

        // Error
        if (level === "error") {
          return (
            <div key={tf} className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/30 opacity-40">
              <span className="font-bold text-sm" style={{ color: degreeColor }}>{TF_LABEL[tf]}</span>
              <span className="text-red-400/60 text-[10px]">Hata</span>
            </div>
          );
        }

        // Has formations — render them
        return (
          <div key={tf}>
            {level.formations.map((f) => (
              <FormationBlock key={f.id} f={f} tf={tf} venue={venue} symbol={symbol} depth={0} />
            ))}
          </div>
        );
      })}
    </div>
  );
}

// ─── Main Page ───────────────────────────────────────────────────────
export function WaveTree() {
  const [venue, setVenue] = useState("binance");
  const [symbol, setSymbol] = useState("BTCUSDT");
  const [activeSymbol, setActiveSymbol] = useState({ venue: "binance", symbol: "BTCUSDT" });
  const [activeOnly, setActiveOnly] = useState(false);

  const handleLoad = () => {
    setActiveSymbol({ venue, symbol });
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
          onKeyDown={(e) => e.key === "Enter" && handleLoad()}
          placeholder="Symbol"
        />
        <button
          onClick={handleLoad}
          className="px-3 py-1 rounded bg-sky-600 hover:bg-sky-500 text-xs text-white font-medium"
        >
          Yukle
        </button>
        <div className="h-4 w-px bg-zinc-700" />
        <button
          onClick={() => setActiveOnly(!activeOnly)}
          className={`px-2.5 py-1 rounded text-[10px] font-medium transition-colors ${
            activeOnly
              ? "bg-emerald-900/50 text-emerald-400 border border-emerald-700/50"
              : "bg-zinc-800 text-zinc-500 border border-zinc-700/50 hover:text-zinc-300"
          }`}
        >
          {activeOnly ? "Sadece Aktif" : "Tumu"}
        </button>
      </div>

      {/* Column headers */}
      <div className="flex items-center gap-2 px-4 py-1 border-b border-zinc-800/50 bg-zinc-900/40 text-[9px] text-zinc-600 uppercase font-medium">
        <span className="w-4" />
        <span className="w-12">TF</span>
        <span className="w-20">Derece</span>
        <span className="flex-1">Formasyon / Dalga</span>
        <span className="w-48 text-right">Fiyat</span>
        <span className="w-12 text-right">Skor</span>
        <span className="w-16 text-right">Tarih</span>
      </div>

      {/* Content — scanner walks through TFs */}
      <div className="flex-1 overflow-auto">
        <TfScanner
          key={`${activeSymbol.venue}_${activeSymbol.symbol}_${activeOnly}`}
          venue={activeSymbol.venue}
          symbol={activeSymbol.symbol}
          activeOnly={activeOnly}
        />
      </div>

      {/* Legend */}
      <div className="flex items-center gap-4 px-4 py-1.5 border-t border-zinc-800 bg-zinc-900/60 text-[9px]">
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-emerald-500" /> Aktif Impulse
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-red-500" /> Aktif Corrective
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-zinc-500" /> Completed
        </span>
        <span className="text-zinc-600 ml-2">Tiklayarak alt periyoda in</span>
      </div>
    </div>
  );
}

export default WaveTree;
