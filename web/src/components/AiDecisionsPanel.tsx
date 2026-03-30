import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchAiDecisions,
  fetchAiPortfolioDirective,
  fetchAiTacticalDirective,
  postAiDecisionApprove,
  postAiDecisionReject,
  type AiDecisionListRowApi,
} from "../api/client";

type Props = {
  accessToken: string | null;
  canAdmin: boolean;
};

export function AiDecisionsPanel({ accessToken, canAdmin }: Props) {
  const [rows, setRows] = useState<AiDecisionListRowApi[]>([]);
  const [portfolio, setPortfolio] = useState<unknown>(null);
  const [tacticalSym, setTacticalSym] = useState("BTCUSDT");
  const [tacticalPreview, setTacticalPreview] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [layerFilter, setLayerFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setErr("");
    setBusy(true);
    try {
      const list = await fetchAiDecisions(accessToken, {
        layer: layerFilter.trim() || undefined,
        status: statusFilter.trim() || undefined,
        limit: 100,
      });
      setRows(list);
      const p = await fetchAiPortfolioDirective(accessToken);
      setPortfolio(p);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, layerFilter, statusFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const loadTactical = async () => {
    if (!accessToken || !tacticalSym.trim()) return;
    setErr("");
    try {
      const t = await fetchAiTacticalDirective(accessToken, tacticalSym.trim());
      setTacticalPreview(t);
    } catch (e) {
      setErr(String(e));
    }
  };

  const act = async (id: string, kind: "approve" | "reject") => {
    if (!accessToken || !canAdmin) return;
    setErr("");
    setBusy(true);
    try {
      if (kind === "approve") await postAiDecisionApprove(accessToken, id);
      else await postAiDecisionReject(accessToken, id);
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!accessToken) {
    return <p className="muted">{t("aiDecisions.loginPrompt")}</p>;
  }

  return (
    <div className="card" style={{ marginTop: "1rem" }}>
      <p className="tv-drawer__section-head">{t("aiDecisions.title")}</p>
      {err ? <p className="tv-drawer__error">{err}</p> : null}
      <div className="tv-settings__fields" style={{ marginBottom: "0.75rem" }}>
        <label>
          <span className="muted">{t("aiDecisions.layerFilter")}</span>
          <input
            className="mono"
            value={layerFilter}
            onChange={(e) => setLayerFilter(e.target.value)}
            placeholder="tactical | operational | strategic"
          />
        </label>
        <label>
          <span className="muted">{t("aiDecisions.statusFilter")}</span>
          <input
            className="mono"
            value={statusFilter}
            onChange={(e) => setStatusFilter(e.target.value)}
            placeholder="pending_approval | approved | …"
          />
        </label>
        <button type="button" disabled={busy} onClick={() => void refresh()}>
          {t("aiDecisions.refresh")}
        </button>
      </div>
      <div style={{ overflowX: "auto" }}>
        <table className="tv-data-table">
          <thead>
            <tr>
              <th>{t("aiDecisions.colCreated")}</th>
              <th>{t("aiDecisions.colLayer")}</th>
              <th>{t("aiDecisions.colSymbol")}</th>
              <th>{t("aiDecisions.colStatus")}</th>
              <th>{t("aiDecisions.colConf")}</th>
              <th>{t("aiDecisions.colModel")}</th>
              {canAdmin ? <th>{t("aiDecisions.colAction")}</th> : null}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.id}>
                <td className="mono">{r.created_at?.slice(0, 19) ?? ""}</td>
                <td>{r.layer}</td>
                <td className="mono">{r.symbol ?? "—"}</td>
                <td>{r.status}</td>
                <td>{r.confidence != null ? r.confidence.toFixed(3) : "—"}</td>
                <td className="mono">{r.model_id ?? "—"}</td>
                {canAdmin ? (
                  <td>
                    {r.status === "pending_approval" ? (
                      <>
                        <button type="button" disabled={busy} onClick={() => void act(r.id, "approve")}>
                          {t("aiDecisions.approve")}
                        </button>{" "}
                        <button type="button" disabled={busy} onClick={() => void act(r.id, "reject")}>
                          {t("aiDecisions.reject")}
                        </button>
                      </>
                    ) : (
                      "—"
                    )}
                  </td>
                ) : null}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="tv-drawer__section-head" style={{ marginTop: "1rem" }}>
        {t("aiDecisions.portfolio")}
      </p>
      <pre className="mono" style={{ fontSize: "0.8rem", maxHeight: 200, overflow: "auto" }}>
        {JSON.stringify(portfolio, null, 2)}
      </pre>
      <p className="tv-drawer__section-head" style={{ marginTop: "1rem" }}>
        {t("aiDecisions.tacticalSection")}
      </p>
      <div className="tv-settings__fields">
        <input className="mono" value={tacticalSym} onChange={(e) => setTacticalSym(e.target.value)} />
        <button type="button" onClick={() => void loadTactical()}>
          {t("aiDecisions.load")}
        </button>
      </div>
      <pre className="mono" style={{ fontSize: "0.8rem", maxHeight: 200, overflow: "auto" }}>
        {JSON.stringify(tacticalPreview, null, 2)}
      </pre>
    </div>
  );
}
