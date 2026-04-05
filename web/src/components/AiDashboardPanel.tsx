import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchAiDecisions,
  fetchAiPortfolioDirective,
  type AiDecisionListRowApi,
} from "../api/client";

type Props = {
  accessToken: string | null;
};

type LayerStats = {
  total: number;
  approved: number;
  rejected: number;
  pending: number;
  error: number;
  applied: number;
  avgConfidence: number;
};

function computeLayerStats(rows: AiDecisionListRowApi[], layer: string): LayerStats {
  const filtered = rows.filter((r) => r.layer === layer);
  const approved = filtered.filter((r) => r.status === "approved").length;
  const rejected = filtered.filter((r) => r.status === "rejected").length;
  const pending = filtered.filter((r) => r.status === "pending_approval").length;
  const error = filtered.filter((r) => r.status === "error").length;
  const applied = filtered.filter((r) => r.status === "applied").length;
  const confs = filtered.map((r) => r.confidence).filter((c): c is number => c != null);
  const avgConfidence = confs.length > 0 ? confs.reduce((a, b) => a + b, 0) / confs.length : 0;
  return { total: filtered.length, approved, rejected, pending, error, applied, avgConfidence };
}

export function AiDashboardPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<AiDecisionListRowApi[]>([]);
  const [portfolio, setPortfolio] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setErr("");
    setBusy(true);
    try {
      const list = await fetchAiDecisions(accessToken, { limit: 200 });
      setRows(list);
      try {
        const p = await fetchAiPortfolioDirective(accessToken);
        setPortfolio(p as Record<string, unknown>);
      } catch {
        setPortfolio(null);
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!accessToken) {
    return <p className="muted">{t("ai.loginPrompt")}</p>;
  }

  const tactical = computeLayerStats(rows, "tactical");
  const operational = computeLayerStats(rows, "operational");
  const strategic = computeLayerStats(rows, "strategic");

  const pendingTotal = tactical.pending + operational.pending + strategic.pending;

  return (
    <div className="card" style={{ marginTop: "0.5rem" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <p className="tv-drawer__section-head">{t("ai.dashboard.title")}</p>
        <button type="button" disabled={busy} onClick={() => void refresh()} style={{ fontSize: "0.75rem" }}>
          {t("ai.refresh")}
        </button>
      </div>
      {err ? <p className="tv-drawer__error">{err}</p> : null}

      {pendingTotal > 0 ? (
        <div
          style={{
            background: "var(--warning-bg, #7c6a2a)",
            color: "var(--warning-fg, #fff)",
            padding: "0.5rem 0.75rem",
            borderRadius: 6,
            marginBottom: "0.75rem",
            fontSize: "0.85rem",
            fontWeight: 600,
          }}
        >
          {t("ai.dashboard.pendingAlert", { count: pendingTotal })}
        </div>
      ) : null}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "0.5rem", marginBottom: "1rem" }}>
        <LayerCard label={t("ai.dashboard.tactical")} stats={tactical} color="#4fc3f7" />
        <LayerCard label={t("ai.dashboard.operational")} stats={operational} color="#aed581" />
        <LayerCard label={t("ai.dashboard.strategic")} stats={strategic} color="#ce93d8" />
      </div>

      {!busy && !err && rows.length === 0 ? (
        <p className="muted" style={{ fontSize: "0.78rem", lineHeight: 1.5, marginBottom: "0.85rem" }}>
          {t("ai.dashboard.emptyHint")}
        </p>
      ) : null}

      {portfolio ? (
        <div style={{ marginTop: "0.5rem" }}>
          <p className="tv-drawer__section-head">{t("ai.dashboard.activePortfolio")}</p>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.4rem", fontSize: "0.8rem" }}>
            <div>
              <span className="muted">{t("ai.dashboard.riskBudget")}: </span>
              <strong>{portfolio.risk_budget_pct != null ? `${portfolio.risk_budget_pct}%` : "—"}</strong>
            </div>
            <div>
              <span className="muted">{t("ai.dashboard.maxPositions")}: </span>
              <strong>{portfolio.max_open_positions ?? "—"}</strong>
            </div>
            <div>
              <span className="muted">{t("ai.dashboard.regime")}: </span>
              <strong>{(portfolio.preferred_regime as string) ?? "—"}</strong>
            </div>
            <div>
              <span className="muted">{t("ai.dashboard.validUntil")}: </span>
              <strong className="mono">
                {portfolio.valid_until ? String(portfolio.valid_until).slice(0, 16) : "—"}
              </strong>
            </div>
          </div>
          {portfolio.macro_note ? (
            <p style={{ fontSize: "0.75rem", marginTop: "0.3rem", fontStyle: "italic" }}>
              {String(portfolio.macro_note)}
            </p>
          ) : null}
        </div>
      ) : null}

      <div style={{ marginTop: "0.75rem" }}>
        <p className="tv-drawer__section-head">{t("ai.dashboard.recentDecisions")}</p>
        <div style={{ overflowX: "auto", maxHeight: 250 }}>
          <table className="tv-data-table" style={{ fontSize: "0.75rem" }}>
            <thead>
              <tr>
                <th>{t("ai.col.time")}</th>
                <th>{t("ai.col.layer")}</th>
                <th>{t("ai.col.symbol")}</th>
                <th>{t("ai.col.status")}</th>
                <th>{t("ai.col.confidence")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.slice(0, 15).map((r) => (
                <tr key={r.id}>
                  <td className="mono">{r.created_at?.slice(11, 19) ?? ""}</td>
                  <td>
                    <span
                      style={{
                        color:
                          r.layer === "tactical" ? "#4fc3f7" : r.layer === "operational" ? "#aed581" : "#ce93d8",
                      }}
                    >
                      {r.layer}
                    </span>
                  </td>
                  <td className="mono">{r.symbol ?? "—"}</td>
                  <td>
                    <StatusBadge status={r.status} />
                  </td>
                  <td>{r.confidence != null ? r.confidence.toFixed(2) : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

function LayerCard({ label, stats, color }: { label: string; stats: LayerStats; color: string }) {
  return (
    <div
      style={{
        border: `1px solid ${color}44`,
        borderRadius: 8,
        padding: "0.6rem",
        background: `${color}0a`,
      }}
    >
      <div style={{ fontWeight: 700, color, fontSize: "0.85rem", marginBottom: "0.3rem" }}>{label}</div>
      <div style={{ fontSize: "0.75rem", lineHeight: 1.6 }}>
        <div>
          <span className="muted">Toplam: </span>
          <strong>{stats.total}</strong>
        </div>
        <div>
          <span className="muted">Onaylanan: </span>
          <span style={{ color: "#66bb6a" }}>{stats.approved}</span>
        </div>
        <div>
          <span className="muted">Bekleyen: </span>
          <span style={{ color: "#ffa726" }}>{stats.pending}</span>
        </div>
        <div>
          <span className="muted">Uygulanan: </span>
          <span style={{ color: "#4fc3f7" }}>{stats.applied}</span>
        </div>
        <div>
          <span className="muted">Hata: </span>
          <span style={{ color: stats.error > 0 ? "#ef5350" : "inherit" }}>{stats.error}</span>
        </div>
        <div>
          <span className="muted">Ort. Güven: </span>
          <strong>{stats.avgConfidence.toFixed(2)}</strong>
        </div>
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const colorMap: Record<string, string> = {
    approved: "#66bb6a",
    applied: "#4fc3f7",
    pending_approval: "#ffa726",
    rejected: "#ef5350",
    error: "#ef5350",
    expired: "#9e9e9e",
  };
  const c = colorMap[status] ?? "#9e9e9e";
  return (
    <span
      style={{
        color: c,
        fontSize: "0.7rem",
        fontWeight: 600,
      }}
    >
      {status}
    </span>
  );
}
