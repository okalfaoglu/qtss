import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchAiDecisions,
  fetchAiDecisionDetail,
  type AiDecisionListRowApi,
  type AiDecisionDetailRowApi,
} from "../api/client";

type Props = {
  accessToken: string | null;
};

type SymbolPerf = {
  symbol: string;
  total: number;
  approved: number;
  applied: number;
  errors: number;
  avgConfidence: number;
  lastDecision: string;
  lastDirection: string;
};

function aggregateBySymbol(rows: AiDecisionListRowApi[]): SymbolPerf[] {
  const map = new Map<
    string,
    { total: number; approved: number; applied: number; errors: number; confs: number[]; lastAt: string; lastDir: string }
  >();

  for (const r of rows) {
    const sym = r.symbol ?? "PORTFOLIO";
    let entry = map.get(sym);
    if (!entry) {
      entry = { total: 0, approved: 0, applied: 0, errors: 0, confs: [], lastAt: "", lastDir: "" };
      map.set(sym, entry);
    }
    entry.total++;
    if (r.status === "approved") entry.approved++;
    if (r.status === "applied") entry.applied++;
    if (r.status === "error") entry.errors++;
    if (r.confidence != null) entry.confs.push(r.confidence);
    if (!entry.lastAt || r.created_at > entry.lastAt) {
      entry.lastAt = r.created_at;
    }
  }

  return Array.from(map.entries())
    .map(([symbol, e]) => ({
      symbol,
      total: e.total,
      approved: e.approved,
      applied: e.applied,
      errors: e.errors,
      avgConfidence: e.confs.length > 0 ? e.confs.reduce((a, b) => a + b, 0) / e.confs.length : 0,
      lastDecision: e.lastAt,
      lastDirection: e.lastDir,
    }))
    .sort((a, b) => b.total - a.total);
}

export function AiPerformancePanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<AiDecisionListRowApi[]>([]);
  const [detail, setDetail] = useState<AiDecisionDetailRowApi | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [layerFilter, setLayerFilter] = useState("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setErr("");
    setBusy(true);
    try {
      const list = await fetchAiDecisions(accessToken, {
        layer: layerFilter.trim() || undefined,
        limit: 300,
      });
      setRows(list);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, layerFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const loadDetail = async (id: string) => {
    if (!accessToken) return;
    setSelectedId(id);
    try {
      const d = await fetchAiDecisionDetail(accessToken, id);
      setDetail(d);
    } catch (e) {
      setErr(String(e));
    }
  };

  if (!accessToken) {
    return <p className="muted">{t("ai.loginPrompt")}</p>;
  }

  const symbolPerfs = aggregateBySymbol(rows);

  const totalDecisions = rows.length;
  const errorRate = totalDecisions > 0 ? (rows.filter((r) => r.status === "error").length / totalDecisions) * 100 : 0;
  const approvalRate =
    totalDecisions > 0
      ? ((rows.filter((r) => r.status === "approved" || r.status === "applied").length / totalDecisions) * 100)
      : 0;

  return (
    <div className="card" style={{ marginTop: "0.5rem" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <p className="tv-drawer__section-head">{t("ai.performance.title")}</p>
        <button type="button" disabled={busy} onClick={() => void refresh()} style={{ fontSize: "0.75rem" }}>
          {t("ai.refresh")}
        </button>
      </div>
      {err ? <p className="tv-drawer__error">{err}</p> : null}

      <div className="tv-settings__fields" style={{ marginBottom: "0.75rem" }}>
        <label>
          <span className="muted">{t("ai.col.layer")}</span>
          <select value={layerFilter} onChange={(e) => setLayerFilter(e.target.value)}>
            <option value="">{t("ai.performance.allLayers")}</option>
            <option value="tactical">Tactical</option>
            <option value="operational">Operational</option>
            <option value="strategic">Strategic</option>
          </select>
        </label>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr 1fr",
          gap: "0.5rem",
          marginBottom: "1rem",
          fontSize: "0.8rem",
        }}
      >
        <div style={{ textAlign: "center", padding: "0.5rem", borderRadius: 6, background: "var(--card-bg, #1a1a2e)" }}>
          <div className="muted">{t("ai.performance.totalDecisions")}</div>
          <div style={{ fontSize: "1.2rem", fontWeight: 700 }}>{totalDecisions}</div>
        </div>
        <div style={{ textAlign: "center", padding: "0.5rem", borderRadius: 6, background: "var(--card-bg, #1a1a2e)" }}>
          <div className="muted">{t("ai.performance.approvalRate")}</div>
          <div style={{ fontSize: "1.2rem", fontWeight: 700, color: "#66bb6a" }}>{approvalRate.toFixed(1)}%</div>
        </div>
        <div style={{ textAlign: "center", padding: "0.5rem", borderRadius: 6, background: "var(--card-bg, #1a1a2e)" }}>
          <div className="muted">{t("ai.performance.errorRate")}</div>
          <div style={{ fontSize: "1.2rem", fontWeight: 700, color: errorRate > 10 ? "#ef5350" : "#66bb6a" }}>
            {errorRate.toFixed(1)}%
          </div>
        </div>
      </div>

      {!busy && !err && rows.length === 0 ? (
        <p className="muted" style={{ fontSize: "0.78rem", lineHeight: 1.5, marginBottom: "0.85rem" }}>
          {t("ai.dashboard.emptyHint")}
        </p>
      ) : null}

      <p className="tv-drawer__section-head">{t("ai.performance.bySymbol")}</p>
      <div style={{ overflowX: "auto", maxHeight: 300 }}>
        <table className="tv-data-table" style={{ fontSize: "0.75rem" }}>
          <thead>
            <tr>
              <th>{t("ai.col.symbol")}</th>
              <th>{t("ai.performance.total")}</th>
              <th>{t("ai.performance.approved")}</th>
              <th>{t("ai.performance.applied")}</th>
              <th>{t("ai.performance.errors")}</th>
              <th>{t("ai.col.confidence")}</th>
              <th>{t("ai.performance.last")}</th>
            </tr>
          </thead>
          <tbody>
            {symbolPerfs.map((s) => (
              <tr key={s.symbol}>
                <td className="mono" style={{ fontWeight: 600 }}>
                  {s.symbol}
                </td>
                <td>{s.total}</td>
                <td style={{ color: "#66bb6a" }}>{s.approved}</td>
                <td style={{ color: "#4fc3f7" }}>{s.applied}</td>
                <td style={{ color: s.errors > 0 ? "#ef5350" : "inherit" }}>{s.errors}</td>
                <td>{s.avgConfidence.toFixed(2)}</td>
                <td className="mono">{s.lastDecision?.slice(11, 19) ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <p className="tv-drawer__section-head" style={{ marginTop: "1rem" }}>
        {t("ai.performance.decisionDetail")}
      </p>
      <p className="muted" style={{ fontSize: "0.7rem", marginBottom: "0.3rem" }}>
        {t("ai.performance.clickToLoad")}
      </p>
      <div style={{ overflowX: "auto", maxHeight: 200 }}>
        <table className="tv-data-table" style={{ fontSize: "0.7rem" }}>
          <thead>
            <tr>
              <th>{t("ai.col.time")}</th>
              <th>{t("ai.col.symbol")}</th>
              <th>{t("ai.col.status")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.slice(0, 30).map((r) => (
              <tr key={r.id} style={{ cursor: "pointer" }} onClick={() => void loadDetail(r.id)}>
                <td className="mono">{r.created_at?.slice(0, 19) ?? ""}</td>
                <td className="mono">{r.symbol ?? "—"}</td>
                <td>{r.status}</td>
                <td>{selectedId === r.id ? "^" : ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {detail && selectedId ? (
        <div style={{ marginTop: "0.5rem" }}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.3rem", fontSize: "0.75rem" }}>
            <div>
              <span className="muted">Model: </span>
              <span className="mono">{detail.model_id ?? "—"}</span>
            </div>
            <div>
              <span className="muted">Confidence: </span>
              <span>{detail.confidence?.toFixed(3) ?? "—"}</span>
            </div>
            <div>
              <span className="muted">Approved: </span>
              <span className="mono">{detail.approved_at?.slice(0, 19) ?? "—"}</span>
            </div>
            <div>
              <span className="muted">Applied: </span>
              <span className="mono">{detail.applied_at?.slice(0, 19) ?? "—"}</span>
            </div>
          </div>
          {detail.raw_output ? (
            <>
              <p className="muted" style={{ fontSize: "0.65rem", marginTop: "0.3rem" }}>
                {t("ai.performance.rawOutput")}
              </p>
              <pre
                className="mono"
                style={{
                  fontSize: "0.65rem",
                  maxHeight: 200,
                  overflow: "auto",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {detail.raw_output}
              </pre>
            </>
          ) : null}
          <p className="muted" style={{ fontSize: "0.65rem", marginTop: "0.3rem" }}>parsed_decision</p>
          <pre className="mono" style={{ fontSize: "0.65rem", maxHeight: 120, overflow: "auto" }}>
            {JSON.stringify(detail.parsed_decision, null, 2)}
          </pre>
          <p className="muted" style={{ fontSize: "0.65rem", marginTop: "0.3rem" }}>meta_json</p>
          <pre className="mono" style={{ fontSize: "0.65rem", maxHeight: 80, overflow: "auto" }}>
            {JSON.stringify(detail.meta_json, null, 2)}
          </pre>
        </div>
      ) : null}
    </div>
  );
}
