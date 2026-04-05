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

interface AlertRule {
  id: string;
  symbol: string; // "*" = all
  direction: "bottom" | "top" | "both";
  minScore: number;
  minSignal: string;
  enabled: boolean;
}

interface ScoreDelta {
  prevBottom: number;
  prevTop: number;
  deltaBottom: number;
  deltaTop: number;
}

type SortField = "score" | "symbol" | "interval" | "time" | "delta";
type SortDir = "asc" | "desc";
type ViewMode = "cards" | "heatmap" | "compare";
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

/* ══════════════════════════ Radar Chart (SVG) ══════════════════════════ */

const PILLAR_LABELS = ["Momentum", "Volume", "Structure", "Onchain"];
const PILLAR_COLORS: Record<string, string> = { Momentum: "#818cf8", Volume: "#38bdf8", Structure: "#f472b6", Onchain: "#34d399" };

function RadarChart({ pillars, size = 120, label }: { pillars: PillarScore[]; size?: number; label?: string }) {
  const cx = size / 2;
  const cy = size / 2;
  const r = size / 2 - 16;
  const levels = [25, 50, 75, 100];

  // Map pillar scores to radar positions (4 axes)
  const pillarMap: Record<string, number> = {};
  for (const p of pillars) pillarMap[p.kind] = p.score;

  const angles = PILLAR_LABELS.map((_, i) => (Math.PI * 2 * i) / 4 - Math.PI / 2);
  const points = PILLAR_LABELS.map((kind, i) => {
    const score = pillarMap[kind] ?? 0;
    const pct = score / 100;
    return { x: cx + r * pct * Math.cos(angles[i]), y: cy + r * pct * Math.sin(angles[i]) };
  });
  const pathD = points.map((p, i) => `${i === 0 ? "M" : "L"} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ") + " Z";

  return (
    <div style={{ textAlign: "center" }}>
      {label && <div style={{ fontSize: 10, color: "#94a3b8", marginBottom: 2 }}>{label}</div>}
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        {/* Grid rings */}
        {levels.map(lv => (
          <polygon
            key={lv}
            points={angles.map(a => `${cx + r * (lv / 100) * Math.cos(a)},${cy + r * (lv / 100) * Math.sin(a)}`).join(" ")}
            fill="none" stroke="#334155" strokeWidth={0.5}
          />
        ))}
        {/* Axis lines */}
        {angles.map((a, i) => (
          <line key={i} x1={cx} y1={cy} x2={cx + r * Math.cos(a)} y2={cy + r * Math.sin(a)} stroke="#334155" strokeWidth={0.5} />
        ))}
        {/* Score polygon */}
        <polygon points={points.map(p => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ")} fill="#818cf833" stroke="#818cf8" strokeWidth={1.5} />
        {/* Score dots */}
        {points.map((p, i) => (
          <circle key={i} cx={p.x} cy={p.y} r={2.5} fill={PILLAR_COLORS[PILLAR_LABELS[i]]} />
        ))}
        {/* Labels */}
        {PILLAR_LABELS.map((kind, i) => {
          const lx = cx + (r + 12) * Math.cos(angles[i]);
          const ly = cy + (r + 12) * Math.sin(angles[i]);
          return (
            <text key={kind} x={lx} y={ly} textAnchor="middle" dominantBaseline="middle" fontSize={8} fill={PILLAR_COLORS[kind]}>
              {kind.slice(0, 3).toUpperCase()}
            </text>
          );
        })}
      </svg>
    </div>
  );
}

/* ══════════════════════════ Delta Badge ══════════════════════════ */

function DeltaBadge({ delta }: { delta: number }) {
  if (Math.abs(delta) < 0.5) return null;
  const up = delta > 0;
  return (
    <span style={{ fontSize: 10, fontWeight: 600, color: up ? "#4ade80" : "#ef4444", marginLeft: 4 }}>
      {up ? "▲" : "▼"}{Math.abs(delta).toFixed(1)}
    </span>
  );
}

/* ══════════════════════════ CSV Export ══════════════════════════ */

function exportCsv(items: SymbolTbm[], deltaMap: Map<string, ScoreDelta>) {
  const header = "Symbol,Exchange,Segment,Interval,Bottom Score,Bottom Signal,Top Score,Top Signal,Delta Bottom,Delta Top,Setups,Computed At";
  const rows = items.map(i => {
    const tbm = i.tbm!;
    const key = `${i.symbol}|${i.interval}`;
    const d = deltaMap.get(key);
    return [
      i.symbol, i.exchange, i.segment, i.interval,
      tbm.bottom.total.toFixed(1), tbm.bottom.signal,
      tbm.top.total.toFixed(1), tbm.top.signal,
      d ? d.deltaBottom.toFixed(1) : "0",
      d ? d.deltaTop.toFixed(1) : "0",
      tbm.setups.length,
      i.computedAt,
    ].join(",");
  });
  const csv = [header, ...rows].join("\n");
  const blob = new Blob([csv], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `tbm_export_${new Date().toISOString().slice(0, 19).replace(/:/g, "")}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

/* ══════════════════════════ Alert Rules (localStorage) ══════════════════════════ */

const ALERT_STORAGE_KEY = "qtss_tbm_alert_rules";

function loadAlertRules(): AlertRule[] {
  try {
    const raw = localStorage.getItem(ALERT_STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch { return []; }
}

function saveAlertRules(rules: AlertRule[]) {
  localStorage.setItem(ALERT_STORAGE_KEY, JSON.stringify(rules));
}

function checkAlerts(items: SymbolTbm[], rules: AlertRule[]): { rule: AlertRule; item: SymbolTbm; score: number; dir: string }[] {
  const triggered: { rule: AlertRule; item: SymbolTbm; score: number; dir: string }[] = [];
  for (const rule of rules) {
    if (!rule.enabled) continue;
    for (const item of items) {
      if (rule.symbol !== "*" && !item.symbol.includes(rule.symbol)) continue;
      const tbm = item.tbm;
      if (!tbm) continue;
      const checkBottom = rule.direction === "bottom" || rule.direction === "both";
      const checkTop = rule.direction === "top" || rule.direction === "both";
      const minSig = SIGNAL_ORDER[rule.minSignal] ?? 0;
      if (checkBottom && tbm.bottom.total >= rule.minScore && (SIGNAL_ORDER[tbm.bottom.signal] ?? 0) >= minSig) {
        triggered.push({ rule, item, score: tbm.bottom.total, dir: "Bottom" });
      }
      if (checkTop && tbm.top.total >= rule.minScore && (SIGNAL_ORDER[tbm.top.signal] ?? 0) >= minSig) {
        triggered.push({ rule, item, score: tbm.top.total, dir: "Top" });
      }
    }
  }
  return triggered;
}

function AlertRulesPanel({ rules, setRules }: { rules: AlertRule[]; setRules: (r: AlertRule[]) => void }) {
  const [newSymbol, setNewSymbol] = useState("*");
  const [newDir, setNewDir] = useState<"bottom" | "top" | "both">("both");
  const [newScore, setNewScore] = useState(60);
  const [newSignal, setNewSignal] = useState("Moderate");

  const addRule = () => {
    const r: AlertRule = { id: Date.now().toString(), symbol: newSymbol, direction: newDir, minScore: newScore, minSignal: newSignal, enabled: true };
    const updated = [...rules, r];
    setRules(updated);
    saveAlertRules(updated);
  };

  const removeRule = (id: string) => {
    const updated = rules.filter(r => r.id !== id);
    setRules(updated);
    saveAlertRules(updated);
  };

  const toggleRule = (id: string) => {
    const updated = rules.map(r => r.id === id ? { ...r, enabled: !r.enabled } : r);
    setRules(updated);
    saveAlertRules(updated);
  };

  const inputStyle: React.CSSProperties = { background: "#1e293b", color: "#e2e8f0", border: "1px solid #334155", borderRadius: 4, padding: "3px 6px", fontSize: 11 };

  return (
    <div style={{ background: "#0f172a", borderRadius: 8, border: "1px solid #334155", padding: 12, marginBottom: 12 }}>
      <div style={{ fontSize: 12, fontWeight: 700, marginBottom: 8 }}>Alert Kurallari</div>

      {/* Existing rules */}
      {rules.length > 0 && (
        <div style={{ marginBottom: 8 }}>
          {rules.map(r => (
            <div key={r.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 0", borderBottom: "1px solid #1e293b", fontSize: 11, opacity: r.enabled ? 1 : 0.4 }}>
              <button type="button" onClick={() => toggleRule(r.id)} style={{ background: "none", border: "none", cursor: "pointer", fontSize: 12, color: r.enabled ? "#4ade80" : "#64748b" }}>
                {r.enabled ? "●" : "○"}
              </button>
              <span style={{ fontWeight: 600 }}>{r.symbol}</span>
              <span style={{ color: "#64748b" }}>{r.direction}</span>
              <span>≥{r.minScore}</span>
              {signalBadge(r.minSignal)}
              <button type="button" onClick={() => removeRule(r.id)} style={{ background: "none", border: "none", cursor: "pointer", color: "#ef4444", fontSize: 12, marginLeft: "auto" }}>✕</button>
            </div>
          ))}
        </div>
      )}

      {/* Add new rule */}
      <div style={{ display: "flex", gap: 6, alignItems: "center", flexWrap: "wrap" }}>
        <input value={newSymbol} onChange={e => setNewSymbol(e.target.value.toUpperCase())} placeholder="* veya BTCUSDT" style={{ ...inputStyle, width: 90 }} />
        <select value={newDir} onChange={e => setNewDir(e.target.value as "bottom" | "top" | "both")} style={inputStyle}>
          <option value="both">Her Iki</option>
          <option value="bottom">Dip</option>
          <option value="top">Tepe</option>
        </select>
        <input type="number" value={newScore} onChange={e => setNewScore(Number(e.target.value))} min={0} max={100} step={5} style={{ ...inputStyle, width: 48 }} />
        <select value={newSignal} onChange={e => setNewSignal(e.target.value)} style={inputStyle}>
          <option value="None">None</option>
          <option value="Weak">Weak</option>
          <option value="Moderate">Moderate</option>
          <option value="Strong">Strong</option>
          <option value="VeryStrong">VeryStrong</option>
        </select>
        <button type="button" onClick={addRule} style={{ background: "#334155", color: "#e2e8f0", border: "1px solid #475569", borderRadius: 4, padding: "3px 10px", fontSize: 11, cursor: "pointer" }}>+ Ekle</button>
      </div>
    </div>
  );
}

/* ══════════════════════════ Compare View ══════════════════════════ */

function CompareView({ items, selectedKeys, setSelectedKeys }: {
  items: SymbolTbm[];
  selectedKeys: Set<string>;
  setSelectedKeys: (s: Set<string>) => void;
}) {
  const selected = items.filter(i => selectedKeys.has(`${i.symbol}|${i.interval}`));

  const toggleSelect = (item: SymbolTbm) => {
    const key = `${item.symbol}|${item.interval}`;
    const next = new Set(selectedKeys);
    if (next.has(key)) next.delete(key);
    else if (next.size < 4) next.add(key); // max 4
    setSelectedKeys(next);
  };

  return (
    <div>
      {/* Selection chips */}
      <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: 10 }}>
        {items.map(i => {
          const key = `${i.symbol}|${i.interval}`;
          const isSelected = selectedKeys.has(key);
          return (
            <button
              key={key}
              type="button"
              onClick={() => toggleSelect(i)}
              style={{
                background: isSelected ? "#334155" : "#0f172a",
                color: isSelected ? "#e2e8f0" : "#64748b",
                border: `1px solid ${isSelected ? "#818cf8" : "#334155"}`,
                borderRadius: 4, padding: "3px 8px", fontSize: 10, cursor: "pointer", fontWeight: isSelected ? 600 : 400,
              }}
            >
              {i.symbol} {i.interval}
            </button>
          );
        })}
      </div>

      {selected.length === 0 && (
        <div style={{ color: "#64748b", fontSize: 12, textAlign: "center", padding: 30 }}>Karsilastirmak icin en fazla 4 sembol secin</div>
      )}

      {/* Comparison grid */}
      {selected.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: `repeat(${Math.min(selected.length, 4)}, 1fr)`, gap: 10 }}>
          {selected.map(item => {
            const tbm = item.tbm!;
            return (
              <div key={`${item.symbol}|${item.interval}`} style={{ background: "#0f172a", borderRadius: 8, padding: 12, border: "1px solid #334155" }}>
                <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 4 }}>{item.symbol} <span style={{ fontSize: 10, color: "#64748b" }}>{item.interval}</span></div>

                {/* Radar */}
                <div style={{ display: "flex", justifyContent: "center", gap: 8 }}>
                  <RadarChart pillars={tbm.bottom.pillars} size={100} label="Bottom" />
                  <RadarChart pillars={tbm.top.pillars} size={100} label="Top" />
                </div>

                {/* Scores */}
                <div style={{ marginTop: 8 }}>
                  {scoreBar(tbm.bottom.total, `Bottom ${tbm.bottom.signal}`, "#4ade80")}
                  {scoreBar(tbm.top.total, `Top ${tbm.top.signal}`, "#f87171")}
                </div>

                {/* Pillar table */}
                <table style={{ width: "100%", fontSize: 10, borderCollapse: "collapse", marginTop: 6 }}>
                  <tbody>
                    {PILLAR_LABELS.map(kind => {
                      const bp = tbm.bottom.pillars.find(p => p.kind === kind);
                      const tp = tbm.top.pillars.find(p => p.kind === kind);
                      return (
                        <tr key={kind} style={{ borderBottom: "1px solid #1e293b" }}>
                          <td style={{ padding: "2px 4px", color: PILLAR_COLORS[kind], fontWeight: 600 }}>{kind.slice(0, 3)}</td>
                          <td style={{ padding: "2px 4px", textAlign: "right", color: "#4ade80" }}>{bp?.score.toFixed(0) ?? "—"}</td>
                          <td style={{ padding: "2px 4px", textAlign: "right", color: "#f87171" }}>{tp?.score.toFixed(0) ?? "—"}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>

                {/* Setups */}
                {tbm.setups.length > 0 && (
                  <div style={{ marginTop: 6, fontSize: 10, background: "#052e1633", borderRadius: 4, padding: "4px 6px", border: "1px solid #22c55e33" }}>
                    {tbm.setups.map((s, i) => (
                      <div key={i}><strong>{s.direction}</strong> {s.signal} ({s.score.toFixed(1)})</div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
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

function SymbolTbmCard({ item, delta }: { item: SymbolTbm; delta?: ScoreDelta }) {
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
          {delta && <DeltaBadge delta={bestDir === "Bottom" ? delta.deltaBottom : delta.deltaTop} />}
        </span>
        <span style={{ fontSize: 11, color: "#475569", transform: expanded ? "rotate(180deg)" : "rotate(0)", transition: "transform 0.2s" }}>&#9660;</span>
      </div>

      <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
        <div style={{ flex: 1 }}>
          {scoreBar(tbm.bottom.total, "Bottom", "#4ade80")}
          {delta && <DeltaBadge delta={delta.deltaBottom} />}
        </div>
        <div style={{ flex: 1 }}>
          {scoreBar(tbm.top.total, "Top", "#f87171")}
          {delta && <DeltaBadge delta={delta.deltaTop} />}
        </div>
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
          {/* Radar Charts */}
          <div style={{ display: "flex", justifyContent: "center", gap: 16, marginBottom: 10 }}>
            <RadarChart pillars={tbm.bottom.pillars} size={120} label="Bottom" />
            <RadarChart pillars={tbm.top.pillars} size={120} label="Top" />
          </div>
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
        <button type="button" style={btnStyle(viewMode === "compare")} onClick={() => setViewMode("compare")}>Karsilastir</button>
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
            <option value="delta">Delta</option>
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
  const [showAlertRules, setShowAlertRules] = useState(false);

  // Delta tracking (in-memory between refreshes)
  const [deltaMap, setDeltaMap] = useState<Map<string, ScoreDelta>>(new Map());
  const prevItemsRef = useRef<SymbolTbm[]>([]);

  // Alert rules (localStorage)
  const [alertRules, setAlertRules] = useState<AlertRule[]>(loadAlertRules);
  const [triggeredAlerts, setTriggeredAlerts] = useState<{ rule: AlertRule; item: SymbolTbm; score: number; dir: string }[]>([]);

  // Compare mode
  const [compareKeys, setCompareKeys] = useState<Set<string>>(new Set());

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

      // Compute deltas from previous items
      const newDeltaMap = new Map<string, ScoreDelta>();
      const prev = prevItemsRef.current;
      for (const item of result) {
        const key = `${item.symbol}|${item.interval}`;
        const prevItem = prev.find(p => p.symbol === item.symbol && p.interval === item.interval);
        if (prevItem?.tbm && item.tbm) {
          newDeltaMap.set(key, {
            prevBottom: prevItem.tbm.bottom.total,
            prevTop: prevItem.tbm.top.total,
            deltaBottom: item.tbm.bottom.total - prevItem.tbm.bottom.total,
            deltaTop: item.tbm.top.total - prevItem.tbm.top.total,
          });
        }
      }
      if (prev.length > 0) setDeltaMap(newDeltaMap);
      prevItemsRef.current = result;

      // Check alert rules
      setTriggeredAlerts(checkAlerts(result, alertRules));

      setItems(result);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
      setCountdown(AUTO_REFRESH_SECS);
    }
  }, [accessToken, alertRules]);

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
        case "delta": {
          const keyA = `${a.symbol}|${a.interval}`;
          const keyB = `${b.symbol}|${b.interval}`;
          const dA = deltaMap.get(keyA);
          const dB = deltaMap.get(keyB);
          const absA = Math.max(Math.abs(dA?.deltaBottom ?? 0), Math.abs(dA?.deltaTop ?? 0));
          const absB = Math.max(Math.abs(dB?.deltaBottom ?? 0), Math.abs(dB?.deltaTop ?? 0));
          cmp = absA - absB;
          break;
        }
        case "symbol": cmp = a.symbol.localeCompare(b.symbol); break;
        case "interval": cmp = TF_ORDER.indexOf(a.interval) - TF_ORDER.indexOf(b.interval); break;
        case "time": cmp = new Date(a.computedAt).getTime() - new Date(b.computedAt).getTime(); break;
      }
      return sortDir === "desc" ? -cmp : cmp;
    });

    return list;
  }, [items, symbolSearch, signalFilter, direction, minScore, sortField, sortDir, deltaMap]);

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

          {/* Alert rules toggle */}
          <button
            type="button"
            onClick={() => setShowAlertRules(!showAlertRules)}
            style={{ background: showAlertRules ? "#334155" : "transparent", color: triggeredAlerts.length > 0 ? "#facc15" : "#e2e8f0", border: `1px solid ${triggeredAlerts.length > 0 ? "#facc15" : "#475569"}`, borderRadius: 4, padding: "4px 10px", fontSize: 11, cursor: "pointer" }}
          >
            Alerts{triggeredAlerts.length > 0 ? ` (${triggeredAlerts.length})` : ""}
          </button>

          {/* Setup log toggle */}
          <button
            type="button"
            onClick={() => setShowSetupLog(!showSetupLog)}
            style={{ background: showSetupLog ? "#334155" : "transparent", color: "#e2e8f0", border: "1px solid #475569", borderRadius: 4, padding: "4px 10px", fontSize: 11, cursor: "pointer" }}
          >
            Setup Log
          </button>

          {/* CSV Export */}
          <button
            type="button"
            onClick={() => exportCsv(filtered, deltaMap)}
            style={{ background: "transparent", color: "#e2e8f0", border: "1px solid #475569", borderRadius: 4, padding: "4px 10px", fontSize: 11, cursor: "pointer" }}
            title="CSV olarak indir"
          >
            CSV
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

      {/* Alert Rules Panel */}
      {showAlertRules && <AlertRulesPanel rules={alertRules} setRules={setAlertRules} />}

      {/* Triggered Alerts Banner */}
      {triggeredAlerts.length > 0 && (
        <div style={{ background: "#422006", border: "1px solid #facc1555", borderRadius: 8, padding: "8px 12px", marginBottom: 12, fontSize: 11 }}>
          <strong style={{ color: "#facc15" }}>Alert ({triggeredAlerts.length}):</strong>{" "}
          {triggeredAlerts.map((a, i) => (
            <span key={i} style={{ marginRight: 8 }}>
              {a.item.symbol} {a.item.interval} {a.dir} {a.score.toFixed(0)}
            </span>
          ))}
        </div>
      )}

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
      ) : viewMode === "compare" ? (
        <CompareView items={filtered} selectedKeys={compareKeys} setSelectedKeys={setCompareKeys} />
      ) : (
        filtered.map((item, i) => (
          <SymbolTbmCard
            key={`${item.symbol}-${item.interval}-${i}`}
            item={item}
            delta={deltaMap.get(`${item.symbol}|${item.interval}`)}
          />
        ))
      )}
    </div>
  );
}
