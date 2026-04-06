import { useCallback, useEffect, useMemo, useState } from "react";
import {
  fetchIntakePlaybookLatest,
  fetchIntakePlaybookRecent,
  postIntakePlaybookPromote,
  type IntakePlaybookCandidateApiRow,
  type IntakePlaybookRunApiRow,
} from "../api/client";

const PLAYBOOK_IDS = [
  "market_mode",
  "elite_short",
  "elite_long",
  "ten_x_alert",
  "institutional_exit",
  "institutional_accumulation",
  "explosive_high_risk",
  "early_accumulation_24h",
] as const;

type Props = {
  accessToken: string | null;
  canPromote: boolean;
  visible: boolean;
};

export function IntakePlaybookPanel({ accessToken, canPromote, visible }: Props) {
  const [playbookId, setPlaybookId] = useState<string>("market_mode");
  const [latest, setLatest] = useState<{
    run: IntakePlaybookRunApiRow | null;
    candidates: IntakePlaybookCandidateApiRow[];
  } | null>(null);
  const [recent, setRecent] = useState<IntakePlaybookRunApiRow[] | null>(null);
  const [err, setErr] = useState("");
  const [loading, setLoading] = useState(false);
  const [promoteId, setPromoteId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!accessToken) {
      setLatest(null);
      setRecent(null);
      return;
    }
    setErr("");
    setLoading(true);
    try {
      const [lat, rec] = await Promise.all([
        fetchIntakePlaybookLatest(accessToken, playbookId),
        fetchIntakePlaybookRecent(accessToken, 25),
      ]);
      setLatest(lat);
      setRecent(rec);
    } catch (e) {
      setLatest(null);
      setRecent(null);
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  }, [accessToken, playbookId]);

  useEffect(() => {
    if (!visible || !accessToken) return;
    void refresh();
  }, [visible, accessToken, refresh]);

  const summaryText = useMemo(() => {
    if (!latest?.run) return "";
    try {
      return JSON.stringify(latest.run.summary_json, null, 2);
    } catch {
      return String(latest.run.summary_json);
    }
  }, [latest]);

  const onPromote = async (candidateId: string) => {
    if (!accessToken || !canPromote) return;
    setPromoteId(candidateId);
    setErr("");
    try {
      await postIntakePlaybookPromote(accessToken, { candidate_id: candidateId });
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setPromoteId(null);
    }
  };

  if (!accessToken) {
    return <p className="muted">Intake playbook için giriş yapın.</p>;
  }

  return (
    <div className="intake-playbook-panel" style={{ marginTop: "0.85rem" }}>
      <p className="tv-drawer__section-head">Intake playbook (smart-money adayları)</p>
      <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
        Worker <code>intake_playbook_engine</code> — tablolar <code>intake_playbook_runs</code> /{" "}
        <code>intake_playbook_candidates</code>. Açmak: <code>QTSS_INTAKE_PLAYBOOK_ENABLED=1</code> veya{" "}
        <code>system_config</code> <code>intake_playbook_loop_enabled</code>.
      </p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "0.35rem", alignItems: "center", marginBottom: "0.5rem" }}>
        <label className="muted" style={{ fontSize: "0.75rem" }}>
          Playbook{" "}
          <select
            className="theme-toggle"
            style={{ marginLeft: "0.25rem", fontSize: "0.75rem" }}
            value={playbookId}
            onChange={(e) => setPlaybookId(e.target.value)}
          >
            {PLAYBOOK_IDS.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </label>
        <button type="button" className="theme-toggle" style={{ fontSize: "0.75rem" }} disabled={loading} onClick={() => void refresh()}>
          {loading ? "Yükleniyor…" : "Yenile"}
        </button>
      </div>
      {err ? <p className="err" style={{ fontSize: "0.78rem" }}>{err}</p> : null}

      {latest?.run ? (
        <>
          <p className="muted mono" style={{ fontSize: "0.68rem", margin: "0.25rem 0" }}>
            {latest.run.computed_at} · mode {latest.run.market_mode ?? "—"} · güven %{latest.run.confidence_0_100} ·{" "}
            {latest.run.key_reason ?? "—"}
          </p>
          {latest.run.neutral_guidance ? (
            <p className="muted" style={{ fontSize: "0.72rem" }}>
              {latest.run.neutral_guidance}
            </p>
          ) : null}
          {summaryText ? (
            <pre
              className="mono muted"
              style={{ fontSize: "0.65rem", maxHeight: "8rem", overflow: "auto", margin: "0.35rem 0" }}
            >
              {summaryText}
            </pre>
          ) : null}
        </>
      ) : !loading ? (
        <p className="muted" style={{ fontSize: "0.75rem" }}>
          Bu playbook için henüz run yok — worker’da intake açık mı ve migration <code>0003_intake_playbook</code> uygulandı
          mı kontrol edin.
        </p>
      ) : null}

      {latest && latest.candidates.length > 0 ? (
        <table className="nansen-api-table" style={{ marginTop: "0.5rem", fontSize: "0.72rem" }}>
          <thead>
            <tr>
              <th>#</th>
              <th>Symbol</th>
              <th>Dir</th>
              <th>Tier</th>
              <th>%</th>
              <th>Promote</th>
            </tr>
          </thead>
          <tbody>
            {latest.candidates.map((c) => (
              <tr key={c.id}>
                <td>{c.rank}</td>
                <td className="mono">{c.symbol}</td>
                <td>{c.direction}</td>
                <td>{c.intake_tier}</td>
                <td>{c.confidence_0_100}</td>
                <td>
                  {c.merged_engine_symbol_id ? (
                    <span className="muted mono" style={{ fontSize: "0.65rem" }}>
                      merged
                    </span>
                  ) : canPromote ? (
                    <button
                      type="button"
                      className="theme-toggle"
                      style={{ fontSize: "0.65rem", padding: "0.15rem 0.35rem" }}
                      disabled={promoteId === c.id}
                      onClick={() => void onPromote(c.id)}
                    >
                      {promoteId === c.id ? "…" : "→ engine"}
                    </button>
                  ) : (
                    <span className="muted">ops</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}

      {recent && recent.length > 0 ? (
        <>
          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
            Son koşular (tüm playbook’lar)
          </p>
          <ul className="muted" style={{ fontSize: "0.68rem", margin: 0, paddingLeft: "1.1rem", lineHeight: 1.5 }}>
            {recent.slice(0, 12).map((r) => (
              <li key={r.id} className="mono">
                {r.playbook_id} · {r.computed_at} · {r.market_mode ?? "—"} · %{r.confidence_0_100}
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </div>
  );
}
