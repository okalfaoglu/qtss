import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchAiApprovalRequests,
  patchAiApprovalRequest,
  postAiApprovalRequest,
  type AiApprovalRequestRowApi,
} from "../api/client";

type Props = {
  accessToken: string | null;
  canOps: boolean;
  canAdmin: boolean;
};

function truncate(s: string, max: number): string {
  const t = s.trim();
  if (t.length <= max) return t;
  return `${t.slice(0, max)}…`;
}

export function OperationsQueuesPanel({ accessToken, canOps, canAdmin }: Props) {
  const { t } = useTranslation();
  const [approvals, setApprovals] = useState<AiApprovalRequestRowApi[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const [aiKind, setAiKind] = useState("generic");
  const [aiPayloadText, setAiPayloadText] = useState('{"note":"dashboard test"}');

  const [approvalFilter, setApprovalFilter] = useState<"" | "pending" | "approved" | "rejected">("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setErr("");
    setBusy(true);
    try {
      const a = await fetchAiApprovalRequests(accessToken, {
        status: approvalFilter || undefined,
        limit: 50,
      });
      setApprovals(a);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, approvalFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const submitAiStub = async () => {
    if (!accessToken || !canOps) return;
    setErr("");
    let payload: unknown;
    try {
      payload = JSON.parse(aiPayloadText) as unknown;
    } catch {
      setErr("AI payload: geçerli JSON girin.");
      return;
    }
    setBusy(true);
    try {
      await postAiApprovalRequest(accessToken, {
        kind: aiKind.trim() || "generic",
        payload,
      });
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const decideApproval = async (id: string, status: "approved" | "rejected") => {
    if (!accessToken || !canAdmin) return;
    setErr("");
    setBusy(true);
    try {
      await patchAiApprovalRequest(accessToken, id, { status });
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!accessToken) {
    return <p className="muted">{t("ai.approvalQueueLoginPrompt")}</p>;
  }

  return (
    <>
      <div className="card">
        <p className="tv-drawer__section-head">{t("ai.approvalQueueHead")}</p>
        <p className="muted" style={{ fontSize: "0.75rem", marginBottom: "0.5rem" }}>
          {t("ai.approvalQueueIntro")}
        </p>
        <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap", marginBottom: "0.65rem" }}>
          <button type="button" className="theme-toggle" disabled={busy} onClick={() => void refresh()}>
            {busy ? "Yenileniyor…" : "Yenile"}
          </button>
          <label className="muted" style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
            AI durum
            <select
              className="mono"
              value={approvalFilter}
              onChange={(e) => setApprovalFilter(e.target.value as typeof approvalFilter)}
              style={{ fontSize: "0.78rem" }}
            >
              <option value="">tümü</option>
              <option value="pending">pending</option>
              <option value="approved">approved</option>
              <option value="rejected">rejected</option>
            </select>
          </label>
        </div>
        {err ? <p className="err" style={{ marginBottom: "0.5rem" }}>{err}</p> : null}
      </div>

      <div className="card">
        <p className="tv-drawer__section-head">AI onay kuyruğu</p>
        <div style={{ overflowX: "auto", maxHeight: "16rem" }}>
          <table style={{ width: "100%", fontSize: "0.68rem", borderCollapse: "collapse" }}>
            <thead>
              <tr className="muted">
                <th style={{ textAlign: "left", padding: "0.2rem" }}>created</th>
                <th style={{ textAlign: "left", padding: "0.2rem" }}>status</th>
                <th style={{ textAlign: "left", padding: "0.2rem" }}>kind</th>
                <th style={{ textAlign: "left", padding: "0.2rem" }}>id</th>
                {canAdmin ? <th style={{ textAlign: "left", padding: "0.2rem" }}>işlem</th> : null}
              </tr>
            </thead>
            <tbody>
              {approvals.length === 0 ? (
                <tr>
                  <td colSpan={canAdmin ? 5 : 4} className="muted" style={{ padding: "0.35rem" }}>
                    Kayıt yok.
                  </td>
                </tr>
              ) : (
                approvals.map((row) => (
                  <tr key={row.id}>
                    <td className="mono" style={{ padding: "0.2rem", whiteSpace: "nowrap" }}>
                      {truncate(row.created_at, 19)}
                    </td>
                    <td style={{ padding: "0.2rem" }}>{row.status}</td>
                    <td style={{ padding: "0.2rem" }}>{row.kind}</td>
                    <td className="mono" style={{ padding: "0.2rem" }} title={row.id}>
                      {truncate(row.id, 8)}…
                    </td>
                    {canAdmin ? (
                      <td style={{ padding: "0.2rem" }}>
                        {row.status === "pending" ? (
                          <span style={{ display: "flex", gap: "0.25rem", flexWrap: "wrap" }}>
                            <button
                              type="button"
                              className="theme-toggle"
                              style={{ fontSize: "0.65rem", padding: "0.15rem 0.35rem" }}
                              disabled={busy}
                              onClick={() => void decideApproval(row.id, "approved")}
                            >
                              onay
                            </button>
                            <button
                              type="button"
                              className="theme-toggle"
                              style={{ fontSize: "0.65rem", padding: "0.15rem 0.35rem" }}
                              disabled={busy}
                              onClick={() => void decideApproval(row.id, "rejected")}
                            >
                              red
                            </button>
                          </span>
                        ) : (
                          <span className="muted">—</span>
                        )}
                      </td>
                    ) : null}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
        {canOps ? (
          <div style={{ marginTop: "0.65rem", display: "flex", flexDirection: "column", gap: "0.4rem" }}>
            <label>
              <span className="muted" style={{ fontSize: "0.72rem" }}>
                kind
              </span>
              <input className="mono" value={aiKind} onChange={(e) => setAiKind(e.target.value)} />
            </label>
            <label>
              <span className="muted" style={{ fontSize: "0.72rem" }}>
                payload (JSON)
              </span>
              <textarea
                className="mono"
                rows={3}
                value={aiPayloadText}
                onChange={(e) => setAiPayloadText(e.target.value)}
                style={{ width: "100%" }}
              />
            </label>
            <button type="button" className="theme-toggle" disabled={busy} onClick={() => void submitAiStub()}>
              AI isteği oluştur (test)
            </button>
          </div>
        ) : (
          <p className="muted" style={{ marginTop: "0.5rem", fontSize: "0.72rem" }}>
            İstek oluşturma: trader veya admin rolü gerekir.
          </p>
        )}
        {!canAdmin ? (
          <p className="muted" style={{ marginTop: "0.45rem", fontSize: "0.72rem" }}>
            Onay/red: yalnız admin rolü (sunucu <code>require_admin</code>).
          </p>
        ) : null}
      </div>
    </>
  );
}
