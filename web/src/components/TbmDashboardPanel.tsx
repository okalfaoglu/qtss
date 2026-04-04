import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchEngineSnapshots, type EngineSnapshotJoinedApiRow } from "../api/client";

/* ── Types ── */

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

/* ── Helpers ── */

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
    <span
      style={{
        background: signalColor(signal),
        color: "#000",
        padding: "1px 6px",
        borderRadius: 4,
        fontSize: 11,
        fontWeight: 600,
      }}
    >
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

/* ── Pillar Card ── */

function PillarCard({ pillar }: { pillar: PillarScore }) {
  const kindLabel: Record<string, string> = {
    Momentum: "Momentum",
    Volume: "Volume",
    Structure: "Structure",
    Onchain: "On-chain",
  };
  const kindColor: Record<string, string> = {
    Momentum: "#818cf8",
    Volume: "#38bdf8",
    Structure: "#f472b6",
    Onchain: "#34d399",
  };
  return (
    <div style={{ background: "#1e293b", borderRadius: 6, padding: "8px 10px", marginBottom: 6, border: `1px solid ${kindColor[pillar.kind] ?? "#555"}33` }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: kindColor[pillar.kind] ?? "#ccc" }}>
          {kindLabel[pillar.kind] ?? pillar.kind}
        </span>
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

/* ── Direction Section (Bottom / Top) ── */

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

/* ── MTF Section ── */

function MtfSection({ mtf }: { mtf: MtfPayload }) {
  return (
    <div style={{ background: "#1e293b", borderRadius: 8, padding: 12, marginTop: 10, border: "1px solid #334155" }}>
      <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 8, display: "flex", alignItems: "center", gap: 8 }}>
        MTF Konfirmasyon
        {mtf.has_conflict && <span style={{ background: "#ef4444", color: "#fff", padding: "1px 6px", borderRadius: 4, fontSize: 10 }}>CONFLICT</span>}
      </div>
      <div style={{ display: "flex", gap: 16, marginBottom: 8 }}>
        <div>
          {scoreBar(mtf.bottom_score, "Bottom", "#4ade80")}
          <div style={{ fontSize: 10, color: "#94a3b8" }}>Alignment: {mtf.bottom_alignment}/{mtf.tf_count} TFs {signalBadge(mtf.bottom_signal)}</div>
        </div>
        <div>
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

/* ── Symbol TBM Card ── */

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
      {/* Header */}
      <div
        style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}
        onClick={() => setExpanded(!expanded)}
      >
        <span style={{ fontSize: 14, fontWeight: 700 }}>{item.symbol}</span>
        <span style={{ fontSize: 11, color: "#64748b" }}>{item.exchange}/{item.segment} {item.interval}</span>
        <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6 }}>
          {signalBadge(bestSignal)}
          <span style={{ fontSize: 16, fontWeight: 800 }}>{bestScore.toFixed(1)}</span>
        </span>
        <span style={{ fontSize: 11, color: "#475569", transform: expanded ? "rotate(180deg)" : "rotate(0)", transition: "transform 0.2s" }}>&#9660;</span>
      </div>

      {/* Quick bars */}
      <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
        <div style={{ flex: 1 }}>{scoreBar(tbm.bottom.total, "Bottom", "#4ade80")}</div>
        <div style={{ flex: 1 }}>{scoreBar(tbm.top.total, "Top", "#f87171")}</div>
      </div>

      {/* Setups */}
      {tbm.setups.length > 0 && (
        <div style={{ marginTop: 6 }}>
          {tbm.setups.map((s, i) => (
            <div key={i} style={{ background: s.direction === "Bottom" ? "#052e1633" : "#2e050533", border: `1px solid ${s.direction === "Bottom" ? "#22c55e44" : "#ef444444"}`, borderRadius: 6, padding: "6px 10px", marginBottom: 4, fontSize: 11 }}>
              <strong>{s.direction === "Bottom" ? "DIP" : "TEPE"} SETUP</strong> — {s.signal} (skor: {s.score.toFixed(1)})
            </div>
          ))}
        </div>
      )}

      {/* Expanded details */}
      {expanded && (
        <div style={{ marginTop: 10 }}>
          <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
            <DirectionSection label="Bottom (Dip)" emoji="&#x1F7E2;" data={tbm.bottom} />
            <DirectionSection label="Top (Tepe)" emoji="&#x1F534;" data={tbm.top} />
          </div>
          {mtf && <MtfSection mtf={mtf} />}
          <div style={{ fontSize: 10, color: "#475569", marginTop: 6 }}>
            {item.computedAt} | {tbm.bar_count} bar
          </div>
        </div>
      )}
    </div>
  );
}

/* ── Main Panel ── */

interface Props {
  accessToken: string;
}

export function TbmDashboardPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [items, setItems] = useState<SymbolTbm[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setBusy(true);
    setErr("");
    try {
      const snaps = await fetchEngineSnapshots(accessToken);

      // TBM ve MTF snapshot'larını grupla
      const tbmMap = new Map<string, { tbm: TbmScorePayload | null; mtf: MtfPayload | null; row: EngineSnapshotJoinedApiRow }>();

      for (const s of snaps) {
        if (s.engine_kind !== "tbm_scores" && s.engine_kind !== "tbm_mtf") continue;
        const key = `${s.symbol}|${s.exchange}|${s.segment}|${s.interval}`;
        if (!tbmMap.has(key)) {
          tbmMap.set(key, { tbm: null, mtf: null, row: s });
        }
        const entry = tbmMap.get(key)!;
        if (s.engine_kind === "tbm_scores") {
          entry.tbm = s.payload as unknown as TbmScorePayload;
          entry.row = s;
        } else if (s.engine_kind === "tbm_mtf") {
          entry.mtf = s.payload as unknown as MtfPayload;
        }
      }

      const result: SymbolTbm[] = [];
      for (const [, v] of tbmMap) {
        if (!v.tbm) continue;
        result.push({
          symbol: v.row.symbol,
          exchange: v.row.exchange,
          segment: v.row.segment,
          interval: v.row.interval,
          tbm: v.tbm,
          mtf: v.mtf,
          computedAt: v.row.computed_at ?? "",
        });
      }

      // En yüksek skor önce
      result.sort((a, b) => {
        const sa = Math.max(a.tbm?.bottom.total ?? 0, a.tbm?.top.total ?? 0);
        const sb = Math.max(b.tbm?.bottom.total ?? 0, b.tbm?.top.total ?? 0);
        return sb - sa;
      });

      setItems(result);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken]);

  useEffect(() => { refresh(); }, [refresh]);

  const activeSetups = items.filter(i => i.tbm && i.tbm.setups.length > 0);

  return (
    <div style={{ padding: 12 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h3 style={{ margin: 0, fontSize: 15 }}>
          TBM Dashboard
        </h3>
        <button
          type="button"
          onClick={refresh}
          disabled={busy}
          style={{
            background: "#334155",
            color: "#e2e8f0",
            border: "1px solid #475569",
            borderRadius: 4,
            padding: "4px 12px",
            fontSize: 12,
            cursor: busy ? "wait" : "pointer",
          }}
        >
          {busy ? "..." : "Yenile"}
        </button>
      </div>

      {err && <div style={{ color: "#ef4444", fontSize: 12, marginBottom: 8 }}>{err}</div>}

      {/* Active setups summary */}
      {activeSetups.length > 0 && (
        <div style={{ background: "#052e16", border: "1px solid #22c55e55", borderRadius: 8, padding: "8px 12px", marginBottom: 12, fontSize: 12 }}>
          <strong>{activeSetups.length} aktif setup:</strong>{" "}
          {activeSetups.map(s => {
            const setup = s.tbm!.setups[0];
            const emoji = setup.direction === "Bottom" ? "&#x1F7E2;" : "&#x1F534;";
            return `${s.symbol} (${s.interval}) ${setup.direction}`;
          }).join(", ")}
        </div>
      )}

      {items.length === 0 && !busy && (
        <div style={{ color: "#64748b", fontSize: 13, textAlign: "center", padding: 40 }}>
          TBM verisi bulunamadi. Engine loop aktif mi kontrol edin.
        </div>
      )}

      {items.map((item, i) => (
        <SymbolTbmCard key={`${item.symbol}-${item.interval}-${i}`} item={item} />
      ))}
    </div>
  );
}
