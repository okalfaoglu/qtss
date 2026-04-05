import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchEngineSnapshots, type EngineSnapshotJoinedApiRow } from "../api/client";

/* ══════════════════════════ Types ══════════════════════════ */

interface PillarScore {
  kind: string;
  score: number;
  weight: number;
  details: string[];
}

interface TbmScorePayload {
  bottom: { total: number; signal: string; pillars: PillarScore[] };
  top: { total: number; signal: string; pillars: PillarScore[] };
  setups: { direction: string; score: number; signal: string; summary: string; pillar_details: string[] }[];
  bar_count: number;
}

interface TfScoreEntry {
  timeframe: string;
  bottom_score: number;
  top_score: number;
}

interface MtfPayload {
  bottom_score: number;
  top_score: number;
  bottom_signal: string;
  top_signal: string;
  bottom_alignment: number;
  top_alignment: number;
  tf_count: number;
  has_conflict: boolean;
  details: string[];
  tf_scores: TfScoreEntry[];
}

interface SymbolTbm {
  symbol: string;
  exchange: string;
  segment: string;
  interval: string;
  tbm: TbmScorePayload | null;
  mtf: MtfPayload | null;
  computedAt: string;
}

type SortField = "score" | "symbol" | "interval" | "time";
type SortDir = "asc" | "desc";
type ViewMode = "cards" | "heatmap";
type DirectionFilter = "all" | "bottom" | "top";
type SignalFilter = "all" | "VeryStrong" | "Strong" | "Moderate" | "Weak" | "None";

/* ══════════════════════════ Constants ══════════════════════════ */

const SIGNAL_ORDER: Record<string, number> = { VeryStrong: 5, Strong: 4, Moderate: 3, Weak: 2, None: 1 };
const TF_ORDER = ["1m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w", "1M"];
const AUTO_REFRESH_SECS = 60;

/* ══════════════════════════ Helpers ══════════════════════════ */

function signalColor(signal: string): string {
  switch (signal) {
    case "VeryStrong": return "#22c55e";
    case "Strong": return "#4ade80";
    case "Moderate": return "#facc15";
    case "Weak": return "#fb923c";
    default: return "#6b7280";
  }
}

function signalBadge(signal: string) {
  return (
    <span style={{ background: signalColor(signal), color: "#000", padding: "1px 6px", borderRadius: 4, fontSize: 11, fontWeight: 600 }}>
      {signal}
    </span>
  );
}

function scoreBar(score: number, label: string, color: string) {
  return (
    <div style={{ marginBottom: 4 }}>
      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, marginBottom: 1 }}>
        <span>{label}</span>
        <span style={{ fontWeight: 600 }}>{score.toFixed(1)}</span>
      </div>
      <div style={{ background: "#1e293b", borderRadius: 3, height: 6, overflow: "hidden" }}>
        <div style={{ width: `${Math.min(score, 100)}%`, height: "100%", background: color, borderRadius: 3 }} />
      </div>
    </div>
  );
}

function heatColor(score: number): string {
  if (score >= 80) return "#22c55e";
  if (score >= 65) return "#4ade80";
  if (score >= 50) return "#facc15";
  if (score >= 35) return "#fb923c";
  if (score >= 20) return "#ef4444";
  return "#334155";
}

function timeAgo(iso: string): string {
  if (!iso) return "—";
  const diff = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

/* ══════════════════════════ Stat Cards ══════════════════════════ */

function StatCard({ label, value, sub, color }: { label: string; value: string | number; sub?: string; color?: string }) {
  return (
    <div style={{ background: "#1e293b", borderRadius: 8, padding: "10px 14px", flex: 1, minWidth: 120, border: "1px solid #334155" }}>
      <div style={{ fontSize: 10, color: "#64748b", textTransform: "uppercase", letterSpacing: 0.5 }}>{label}</div>
      <div style={{ fontSize: 22, fontWeight: 800, color: color ?? "#e2e8f0", marginTop: 2 }}>{value}</div>
      {sub && <div style={{ fontSize: 10, color: "#94a3b8", marginTop: 1 }}>{sub}</div>}
    </div>
  );
}

/* ══════════════════════════ Pillar Card ══════════════════════════ */

function PillarCard({ pillar }: { pillar: PillarScore }) {
  const kindLabel: Record<string, string> = { Momentum: "Momentum", Volume: "Volume", Structure: "Structure", Onchain: "On-chain" };
  const kindColor: Record<string, string> = { Momentum: "#818cf8", Volume: "#38bdf8", Structure: "#f472b6", Onchain: "#34d399" };
  return (
    <div style={{ background: "#1e293b", borderRadius: 6, padding: "8px 10px", marginBottom: 6, border: `1px solid ${kindColor[pillar.kind] ?? "#555"}33` }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: kindColor[pillar.kind] ?? "#ccc" }}>{kindLabel[pillar.kind] ?? pillar.kind}</span>
        <span style={{ fontSize: 12, fontWeight: 700 }}>{pillar.score.toFixed(1)}</span>
      </div>
      <div style={{ background: "#0f172a", borderRadius: 3, height: 4, overflow: "hidden", marginBottom: 4 }}>
        <div style={{ width: `${Math.min(pillar.score, 100)}%`, height: "100%", background: kindColor[pillar.kind] ?? "#888", borderRadius: 3 }} />
      </div>
      {pillar.details.length > 0 && (
        <ul style={{ margin: 0, padding: "0 0 0 14px", fontSize: 10, color: "#94a3b8", lineHeight: 1.5 }}>
          {pillar.details.map((d, i) => <li key={i}>{d}</li>)}
        </ul>
      )}
    </div>
  );
}

/* ══════════════════════════ Direction Section ══════════════════════════ */

function DirectionSection({ label, emoji, data }: { label: string; emoji: string; data: { total: number; signal: string; pillars: PillarScore[] } }) {
  return (
    <div style={{ flex: 1, minWidth: 240 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
        <span style={{ fontSize: 16 }}>{emoji}</span>
        <span style={{ fontSize: 14, fontWeight: 700 }}>{label}</span>
        {signalBadge(data.signal)}
        <span style={{ fontSize: 18, fontWeight: 800, marginLeft: "auto" }}>{data.total.toFixed(1)}</span>
      </div>
      {data.pillars.map((p, i) => <PillarCard key={i} pillar={p} />)}
    </div>
  );
}

/* ══════════════════════════ MTF Section ══════════════════════════ */

function MtfSection({ mtf }: { mtf: MtfPayload }) {
  return (
    <div style={{ background: "#1e293b", borderRadius: 8, padding: 12, marginTop: 10, border: "1px solid #334155" }}>
      <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 8, display: "flex", alignItems: "center", gap: 8 }}>
        MTF Konfirmasyon
        {mtf.has_conflict && <span style={{ background: "#ef4444", color: "#fff", padding: "1px 6px", borderRadius: 4, fontSize: 10 }}>CONFLICT</span>}
      </div>
      <div style={{ display: "flex", gap: 16, marginBottom: 8 }}>
        <div style={{ flex: 1 }}>
          {scoreBar(mtf.bottom_score, "Bottom", "#4ade80")}
          <div style={{ fontSize: 10, color: "#94a3b8" }}>Alignment: {mtf.bottom_alignment}/{mtf.tf_count} TFs {signalBadge(mtf.bottom_signal)}</div>
        </div>
        <div style={{ flex: 1 }}>
          {scoreBar(mtf.top_score, "Top", "#f87171")}
          <div style={{ fontSize: 10, color: "#94a3b8" }}>Alignment: {mtf.top_alignment}/{mtf.tf_count} TFs {signalBadge(mtf.top_signal)}</div>
        </div>
      </div>
      {mtf.tf_scores.length > 0 && (
        <table style={{ width: "100%", fontSize: 11, borderCollapse: "collapse" }}>
          <thead>
            <tr style={{ color: "#64748b", borderBottom: "1px solid #334155" }}>
              <th style={{ textAlign: "left", padding: "2px 6px" }}>TF</th>
              <th style={{ textAlign: "right", padding: "2px 6px" }}>Bottom</th>
              <th style={{ textAlign: "right", padding: "2px 6px" }}>Top</th>
            </tr>
          </thead>
          <tbody>
            {mtf.tf_scores.map((tf, i) => (
              <tr key={i} style={{ borderBottom: "1px solid #1e293b" }}>
                <td style={{ padding: "2px 6px", fontWeight: 600 }}>{tf.timeframe}</td>
                <td style={{ padding: "2px 6px", textAlign: "right", color: tf.bottom_score > 50 ? "#4ade80" : "#94a3b8" }}>{tf.bottom_score.toFixed(1)}</td>
                <td style={{ padding: "2px 6px", textAlign: "right", color: tf.top_score > 50 ? "#f87171" : "#94a3b8" }}>{tf.top_score.toFixed(1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {mtf.details.length > 0 && (
        <ul style={{ margin: "6px 0 0", padding: "0 0 0 14px", fontSize: 10, color: "#94a3b8" }}>
          {mtf.details.map((d, i) => <li key={i}>{d}</li>)}
        </ul>
      )}
    </div>
  );
}

/* ══════════════════════════ Symbol TBM Card ══════════════════════════ */

function SymbolTbmCard({ item }: { item: SymbolTbm }) {
  const [expanded, setExpanded] = useState(false);
  const tbm = item.tbm;
  const mtf = item.mtf;
  if (!tbm) return null;

  const bestScore = Math.max(tbm.bottom.total, tbm.top.total);
  const bestDir = tbm.bottom.total >= tbm.top.total ? "Bottom" : "Top";
  const bestSignal = bestDir === "Bottom" ? tbm.bottom.signal : tbm.top.signal;

  return (
    <div style={{ background: "#0f172a", borderRadius: 8, padding: 12, marginBottom: 10, border: `1px solid ${bestScore >= 70 ? "#22c55e44" : bestScore >= 50 ? "#facc1544" : "#33415544"}` }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }} onClick={() => setExpanded(!expanded)}>
        <span style={{ fontSize: 14, fontWeight: 700 }}>{item.symbol}</span>
        <span style={{ fontSize: 11, color: "#64748b" }}>{item.exchange}/{item.segment} {item.interval}</span>
        <span style={{ fontSize: 10, color: "#475569" }}>{timeAgo(item.computedAt)}</span>
        <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6 }}>
          {signalBadge(bestSignal)}
          <span style={{ fontSize: 16, fontWeight: 800 }}>{bestScore.toFixed(1)}</span>
        </span>
        <span style={{ fontSize: 11, color: "#475569", transform: expanded ? "rotate(180deg)" : "rotate(0)", transition: "transform 0.2s" }}>&#9660;</span>
      </div>

      <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
        <div style={{ flex: 1 }}>{scoreBar(tbm.bottom.total, "Bottom", "#4ade80")}</div>
        <div style={{ flex: 1 }}>{scoreBar(tbm.top.total, "Top", "#f87171")}</div>
      </div>

      {tbm.setups.length > 0 && (
        <div style={{ marginTop: 6 }}>
          {tbm.setups.map((s, i) => (
            <div key={i} style={{ background: s.direction === "Bottom" ? "#052e1633" : "#2e050533", border: `1px solid ${s.direction === "Bottom" ? "#22c55e44" : "#ef444444"}`, borderRadius: 6, padding: "6px 10px", marginBottom: 4, fontSize: 11 }}>
              <strong>{s.direction === "Bottom" ? "DIP" : "TEPE"} SETUP</strong> — {s.signal} (skor: {s.score.toFixed(1)})
            </div>
          ))}
        </div>
      )}

      {expanded && (
        <div style={{ marginTop: 10 }}>
          <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
            <DirectionSection label="Bottom (Dip)" emoji="&#x1F7E2;" data={tbm.bottom} />
            <DirectionSection label="Top (Tepe)" emoji="&#x1F534;" data={tbm.top} />
          </div>
          {mtf && <MtfSection mtf={mtf} />}
          <div style={{ fontSize: 10, color: "#475569", marginTop: 6 }}>{item.computedAt} | {tbm.bar_count} bar</div>
        </div>
      )}
    </div>
  );
}

/* ══════════════════════════ Heatmap View ══════════════════════════ */

function HeatmapView({ items, direction }: { items: SymbolTbm[]; direction: DirectionFilter }) {
  // Build symbol -> interval -> score map
  const symbols = [...new Set(items.map(i => i.symbol))].sort();
  const intervals = [...new Set(items.map(i => i.interval))].sort((a, b) => TF_ORDER.indexOf(a) - TF_ORDER.indexOf(b));

  const scoreMap = new Map<string, SymbolTbm>();
  for (const item of items) {
    scoreMap.set(`${item.symbol}|${item.interval}`, item);
  }

  return (
    <div style={{ overflowX: "auto" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left", padding: "6px 8px", color: "#64748b", borderBottom: "1px solid #334155", position: "sticky", left: 0, background: "#0f172a", zIndex: 1 }}>Symbol</th>
            {intervals.map(iv => (
              <th key={iv} style={{ textAlign: "center", padding: "6px 8px", color: "#64748b", borderBottom: "1px solid #334155", minWidth: 56 }}>{iv}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {symbols.map(sym => (
            <tr key={sym}>
              <td style={{ padding: "4px 8px", fontWeight: 600, borderBottom: "1px solid #1e293b", position: "sticky", left: 0, background: "#0f172a", zIndex: 1 }}>{sym}</td>
              {intervals.map(iv => {
                const item = scoreMap.get(`${sym}|${iv}`);
                const tbm = item?.tbm;
                if (!tbm) {
                  return <td key={iv} style={{ textAlign: "center", padding: "4px 8px", borderBottom: "1px solid #1e293b", color: "#334155" }}>—</td>;
                }
                const bottomScore = tbm.bottom.total;
                const topScore = tbm.top.total;
                const showBottom = direction === "all" || direction === "bottom";
                const showTop = direction === "all" || direction === "top";
                const displayScore = direction === "bottom" ? bottomScore : direction === "top" ? topScore : Math.max(bottomScore, topScore);
                const bg = heatColor(displayScore);
                const hasSetup = tbm.setups.length > 0;

                return (
                  <td key={iv} style={{ textAlign: "center", padding: "4px 4px", borderBottom: "1px solid #1e293b" }}>
                    <div style={{ background: bg + "33", border: `1px solid ${bg}66`, borderRadius: 4, padding: "3px 2px", position: "relative" }} title={`B: ${bottomScore.toFixed(1)} | T: ${topScore.toFixed(1)}`}>
                      {showBottom && showTop ? (
                        <div style={{ display: "flex", justifyContent: "center", gap: 2 }}>
                          <span style={{ color: "#4ade80", fontWeight: 600, fontSize: 10 }}>{bottomScore.toFixed(0)}</span>
                          <span style={{ color: "#475569" }}>/</span>
                          <span style={{ color: "#f87171", fontWeight: 600, fontSize: 10 }}>{topScore.toFixed(0)}</span>
                        </div>
                      ) : (
                        <span style={{ fontWeight: 700, fontSize: 11 }}>{displayScore.toFixed(0)}</span>
                      )}
                      {hasSetup && (
                        <div style={{ position: "absolute", top: -3, right: -3, width: 7, height: 7, borderRadius: "50%", background: tbm.setups[0].direction === "Bottom" ? "#22c55e" : "#ef4444", border: "1px solid #0f172a" }} />
                      )}
                    </div>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* ══════════════════════════ Setup Log ══════════════════════════ */

function SetupLog({ items }: { items: SymbolTbm[] }) {
  const setups = items
    .flatMap(item => (item.tbm?.setups ?? []).map(s => ({ ...s, symbol: item.symbol, interval: item.interval, exchange: item.exchange, segment: item.segment, computedAt: item.computedAt })))
    .sort((a, b) => new Date(b.computedAt).getTime() - new Date(a.computedAt).getTime());

  if (setups.length === 0) {
    return <div style={{ color: "#64748b", fontSize: 12, textAlign: "center", padding: 20 }}>Aktif setup yok</div>;
  }

  return (
    <div style={{ background: "#0f172a", borderRadius: 8, border: "1px solid #334155", overflow: "hidden" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
        <thead>
          <tr style={{ background: "#1e293b" }}>
            <th style={{ textAlign: "left", padding: "6px 10px", color: "#64748b" }}>Zaman</th>
            <th style={{ textAlign: "left", padding: "6px 10px", color: "#64748b" }}>Sembol</th>
            <th style={{ textAlign: "center", padding: "6px 10px", color: "#64748b" }}>TF</th>
            <th style={{ textAlign: "center", padding: "6px 10px", color: "#64748b" }}>Yon</th>
            <th style={{ textAlign: "center", padding: "6px 10px", color: "#64748b" }}>Sinyal</th>
            <th style={{ textAlign: "right", padding: "6px 10px", color: "#64748b" }}>Skor</th>
          </tr>
        </thead>
        <tbody>
          {setups.map((s, i) => (
            <tr key={i} style={{ borderTop: "1px solid #1e293b" }}>
              <td style={{ padding: "5px 10px", color: "#94a3b8" }}>{timeAgo(s.computedAt)}</td>
              <td style={{ padding: "5px 10px", fontWeight: 600 }}>{s.symbol}</td>
              <td style={{ padding: "5px 10px", textAlign: "center" }}>{s.interval}</td>
              <td style={{ padding: "5px 10px", textAlign: "center" }}>
                <span style={{ background: s.direction === "Bottom" ? "#052e16" : "#2e0505", color: s.direction === "Bottom" ? "#4ade80" : "#f87171", padding: "1px 6px", borderRadius: 4, fontSize: 10, fontWeight: 600 }}>
                  {s.direction === "Bottom" ? "DIP" : "TEPE"}
                </span>
              </td>
              <td style={{ padding: "5px 10px", textAlign: "center" }}>{signalBadge(s.signal)}</td>
              <td style={{ padding: "5px 10px", textAlign: "right", fontWeight: 700 }}>{s.score.toFixed(1)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* ══════════════════════════ Filter Bar ══════════════════════════ */

function FilterBar({
  viewMode, setViewMode,
  direction, setDirection,
  signalFilter, setSignalFilter,
  symbolSearch, setSymbolSearch,
  minScore, setMinScore,
  sortField, setSortField,
  sortDir, setSortDir,
}: {
  viewMode: ViewMode; setViewMode: (v: ViewMode) => void;
  direction: DirectionFilter; setDirection: (v: DirectionFilter) => void;
  signalFilter: SignalFilter; setSignalFilter: (v: SignalFilter) => void;
  symbolSearch: string; setSymbolSearch: (v: string) => void;
  minScore: number; setMinScore: (v: number) => void;
  sortField: SortField; setSortField: (v: SortField) => void;
  sortDir: SortDir; setSortDir: (v: SortDir) => void;
}) {
  const selectStyle: React.CSSProperties = {
    background: "#1e293b", color: "#e2e8f0", border: "1px solid #334155", borderRadius: 4, padding: "4px 8px", fontSize: 11, outline: "none",
  };
  const btnStyle = (active: boolean): React.CSSProperties => ({
    background: active ? "#334155" : "transparent", color: active ? "#e2e8f0" : "#64748b", border: "1px solid #334155", borderRadius: 4, padding: "4px 10px", fontSize: 11, cursor: "pointer", fontWeight: active ? 600 : 400,
  });

  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center", marginBottom: 12, padding: "8px 12px", background: "#0f172a", borderRadius: 8, border: "1px solid #1e293b" }}>
      {/* View mode */}
      <div style={{ display: "flex", gap: 2 }}>
        <button type="button" style={btnStyle(viewMode === "cards")} onClick={() => setViewMode("cards")}>Kartlar</button>
        <button type="button" style={btnStyle(viewMode === "heatmap")} onClick={() => setViewMode("heatmap")}>Heatmap</button>
      </div>

      <div style={{ width: 1, height: 20, background: "#334155" }} />

      {/* Direction */}
      <div style={{ display: "flex", gap: 2 }}>
        <button type="button" style={btnStyle(direction === "all")} onClick={() => setDirection("all")}>Tumu</button>
        <button type="button" style={btnStyle(direction === "bottom")} onClick={() => setDirection("bottom")}>Dip</button>
        <button type="button" style={btnStyle(direction === "top")} onClick={() => setDirection("top")}>Tepe</button>
      </div>

      <div style={{ width: 1, height: 20, background: "#334155" }} />

      {/* Signal filter */}
      <select value={signalFilter} onChange={e => setSignalFilter(e.target.value as SignalFilter)} style={selectStyle}>
        <option value="all">Tum Sinyaller</option>
        <option value="VeryStrong">VeryStrong</option>
        <option value="Strong">Strong</option>
        <option value="Moderate">Moderate</option>
        <option value="Weak">Weak</option>
      </select>

      {/* Symbol search */}
      <input
        type="text"
        placeholder="Sembol ara..."
        value={symbolSearch}
        onChange={e => setSymbolSearch(e.target.value.toUpperCase())}
        style={{ ...selectStyle, width: 100 }}
      />

      {/* Min score */}
      <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <span style={{ fontSize: 10, color: "#64748b" }}>Min:</span>
        <input
          type="number"
          min={0}
          max={100}
          step={5}
          value={minScore}
          onChange={e => setMinScore(Number(e.target.value))}
          style={{ ...selectStyle, width: 48, textAlign: "center" }}
        />
      </div>

      <div style={{ flex: 1 }} />

      {/* Sort */}
      {viewMode === "cards" && (
        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <select value={sortField} onChange={e => setSortField(e.target.value as SortField)} style={selectStyle}>
            <option value="score">Skor</option>
            <option value="symbol">Sembol</option>
            <option value="interval">TF</option>
            <option value="time">Zaman</option>
          </select>
          <button type="button" style={btnStyle(false)} onClick={() => setSortDir(sortDir === "asc" ? "desc" : "asc")}>
            {sortDir === "desc" ? "↓" : "↑"}
          </button>
        </div>
      )}
    </div>
  );
}

/* ══════════════════════════ Main Panel ══════════════════════════ */

interface Props {
  accessToken: string;
}

export function TbmDashboardPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [items, setItems] = useState<SymbolTbm[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [countdown, setCountdown] = useState(AUTO_REFRESH_SECS);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [showSetupLog, setShowSetupLog] = useState(false);

  // Filters
  const [viewMode, setViewMode] = useState<ViewMode>("cards");
  const [direction, setDirection] = useState<DirectionFilter>("all");
  const [signalFilter, setSignalFilter] = useState<SignalFilter>("all");
  const [symbolSearch, setSymbolSearch] = useState("");
  const [minScore, setMinScore] = useState(0);
  const [sortField, setSortField] = useState<SortField>("score");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const refreshRef = useRef<() => Promise<void>>();

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setBusy(true);
    setErr("");
    try {
      const snaps = await fetchEngineSnapshots(accessToken);
      const tbmMap = new Map<string, { tbm: TbmScorePayload | null; mtf: MtfPayload | null; row: EngineSnapshotJoinedApiRow }>();

      for (const s of snaps) {
        if (s.engine_kind !== "tbm_scores" && s.engine_kind !== "tbm_mtf") continue;
        const key = `${s.symbol}|${s.exchange}|${s.segment}|${s.interval}`;
        if (!tbmMap.has(key)) tbmMap.set(key, { tbm: null, mtf: null, row: s });
        const entry = tbmMap.get(key)!;
        if (s.engine_kind === "tbm_scores") { entry.tbm = s.payload as unknown as TbmScorePayload; entry.row = s; }
        else if (s.engine_kind === "tbm_mtf") { entry.mtf = s.payload as unknown as MtfPayload; }
      }

      const result: SymbolTbm[] = [];
      for (const [, v] of tbmMap) {
        if (!v.tbm) continue;
        result.push({ symbol: v.row.symbol, exchange: v.row.exchange, segment: v.row.segment, interval: v.row.interval, tbm: v.tbm, mtf: v.mtf, computedAt: v.row.computed_at ?? "" });
      }
      setItems(result);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
      setCountdown(AUTO_REFRESH_SECS);
    }
  }, [accessToken]);

  refreshRef.current = refresh;

  useEffect(() => { refresh(); }, [refresh]);

  // Auto-refresh countdown
  useEffect(() => {
    if (!autoRefresh) return;
    const id = setInterval(() => {
      setCountdown(prev => {
        if (prev <= 1) {
          refreshRef.current?.();
          return AUTO_REFRESH_SECS;
        }
        return prev - 1;
      });
    }, 1000);
    return () => clearInterval(id);
  }, [autoRefresh]);

  // Filtered & sorted items
  const filtered = useMemo(() => {
    let list = [...items];

    // Symbol search
    if (symbolSearch) list = list.filter(i => i.symbol.includes(symbolSearch));

    // Signal filter
    if (signalFilter !== "all") {
      list = list.filter(i => {
        const tbm = i.tbm;
        if (!tbm) return false;
        return tbm.bottom.signal === signalFilter || tbm.top.signal === signalFilter;
      });
    }

    // Direction + min score
    list = list.filter(i => {
      const tbm = i.tbm;
      if (!tbm) return false;
      const score = direction === "bottom" ? tbm.bottom.total : direction === "top" ? tbm.top.total : Math.max(tbm.bottom.total, tbm.top.total);
      return score >= minScore;
    });

    // Sort
    list.sort((a, b) => {
      let cmp = 0;
      const tbmA = a.tbm!, tbmB = b.tbm!;
      switch (sortField) {
        case "score": {
          const sa = direction === "bottom" ? tbmA.bottom.total : direction === "top" ? tbmA.top.total : Math.max(tbmA.bottom.total, tbmA.top.total);
          const sb = direction === "bottom" ? tbmB.bottom.total : direction === "top" ? tbmB.top.total : Math.max(tbmB.bottom.total, tbmB.top.total);
          cmp = sa - sb;
          break;
        }
        case "symbol": cmp = a.symbol.localeCompare(b.symbol); break;
        case "interval": cmp = TF_ORDER.indexOf(a.interval) - TF_ORDER.indexOf(b.interval); break;
        case "time": cmp = new Date(a.computedAt).getTime() - new Date(b.computedAt).getTime(); break;
      }
      return sortDir === "desc" ? -cmp : cmp;
    });

    return list;
  }, [items, symbolSearch, signalFilter, direction, minScore, sortField, sortDir]);

  // Stats
  const stats = useMemo(() => {
    const totalSymbols = new Set(items.map(i => i.symbol)).size;
    const activeSetups = items.filter(i => (i.tbm?.setups.length ?? 0) > 0).length;
    const avgScore = items.length > 0 ? items.reduce((sum, i) => sum + Math.max(i.tbm?.bottom.total ?? 0, i.tbm?.top.total ?? 0), 0) / items.length : 0;
    const strongest = items.reduce((best, i) => {
      const s = Math.max(i.tbm?.bottom.total ?? 0, i.tbm?.top.total ?? 0);
      return s > best.score ? { symbol: i.symbol, interval: i.interval, score: s } : best;
    }, { symbol: "—", interval: "", score: 0 });
    const signalCounts = { VeryStrong: 0, Strong: 0, Moderate: 0, Weak: 0 };
    for (const i of items) {
      const bs = i.tbm?.bottom.signal ?? "None";
      const ts = i.tbm?.top.signal ?? "None";
      const best = (SIGNAL_ORDER[bs] ?? 0) >= (SIGNAL_ORDER[ts] ?? 0) ? bs : ts;
      if (best in signalCounts) signalCounts[best as keyof typeof signalCounts]++;
    }
    return { totalSymbols, activeSetups, avgScore, strongest, signalCounts };
  }, [items]);

  return (
    <div style={{ padding: 12 }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h3 style={{ margin: 0, fontSize: 15, display: "flex", alignItems: "center", gap: 8 }}>
          TBM Dashboard
          <span style={{ fontSize: 10, color: "#64748b", fontWeight: 400 }}>Enterprise</span>
        </h3>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {/* Auto-refresh toggle */}
          <button
            type="button"
            onClick={() => setAutoRefresh(!autoRefresh)}
            style={{ background: "transparent", border: "1px solid #334155", borderRadius: 4, padding: "3px 8px", fontSize: 10, color: autoRefresh ? "#4ade80" : "#64748b", cursor: "pointer" }}
            title={autoRefresh ? "Auto-refresh aktif" : "Auto-refresh kapali"}
          >
            {autoRefresh ? `${countdown}s` : "OFF"}
          </button>

          {/* Setup log toggle */}
          <button
            type="button"
            onClick={() => setShowSetupLog(!showSetupLog)}
            style={{ background: showSetupLog ? "#334155" : "transparent", color: "#e2e8f0", border: "1px solid #475569", borderRadius: 4, padding: "4px 10px", fontSize: 11, cursor: "pointer" }}
          >
            Setup Log
          </button>

          <button
            type="button"
            onClick={refresh}
            disabled={busy}
            style={{ background: "#334155", color: "#e2e8f0", border: "1px solid #475569", borderRadius: 4, padding: "4px 12px", fontSize: 12, cursor: busy ? "wait" : "pointer" }}
          >
            {busy ? "..." : "Yenile"}
          </button>
        </div>
      </div>

      {err && <div style={{ color: "#ef4444", fontSize: 12, marginBottom: 8 }}>{err}</div>}

      {/* Stat Cards */}
      <div style={{ display: "flex", gap: 8, marginBottom: 12, flexWrap: "wrap" }}>
        <StatCard label="Sembol" value={stats.totalSymbols} sub={`${items.length} snapshot`} />
        <StatCard label="Aktif Setup" value={stats.activeSetups} color={stats.activeSetups > 0 ? "#22c55e" : "#64748b"} sub={stats.activeSetups > 0 ? "Dikkat!" : "Yok"} />
        <StatCard label="Ort. Skor" value={stats.avgScore.toFixed(1)} color={stats.avgScore >= 50 ? "#facc15" : "#64748b"} />
        <StatCard
          label="En Guclu"
          value={stats.strongest.score > 0 ? stats.strongest.score.toFixed(1) : "—"}
          sub={stats.strongest.score > 0 ? `${stats.strongest.symbol} ${stats.strongest.interval}` : ""}
          color={stats.strongest.score >= 70 ? "#22c55e" : stats.strongest.score >= 50 ? "#facc15" : "#64748b"}
        />
        <StatCard
          label="Sinyal Dagilimi"
          value=""
          sub={`VS:${stats.signalCounts.VeryStrong} S:${stats.signalCounts.Strong} M:${stats.signalCounts.Moderate} W:${stats.signalCounts.Weak}`}
        />
      </div>

      {/* Setup Log */}
      {showSetupLog && (
        <div style={{ marginBottom: 12 }}>
          <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 6 }}>Setup Log</div>
          <SetupLog items={items} />
        </div>
      )}

      {/* Filter Bar */}
      <FilterBar
        viewMode={viewMode} setViewMode={setViewMode}
        direction={direction} setDirection={setDirection}
        signalFilter={signalFilter} setSignalFilter={setSignalFilter}
        symbolSearch={symbolSearch} setSymbolSearch={setSymbolSearch}
        minScore={minScore} setMinScore={setMinScore}
        sortField={sortField} setSortField={setSortField}
        sortDir={sortDir} setSortDir={setSortDir}
      />

      {/* Content */}
      {filtered.length === 0 && !busy && (
        <div style={{ color: "#64748b", fontSize: 13, textAlign: "center", padding: 40 }}>
          {items.length === 0 ? "TBM verisi bulunamadi. Engine loop aktif mi kontrol edin." : "Filtreye uyan sonuc yok."}
        </div>
      )}

      {viewMode === "heatmap" ? (
        <HeatmapView items={filtered} direction={direction} />
      ) : (
        filtered.map((item, i) => <SymbolTbmCard key={`${item.symbol}-${item.interval}-${i}`} item={item} />)
      )}
    </div>
  );
}
