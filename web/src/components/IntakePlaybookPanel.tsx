import { useCallback, useEffect, useMemo, useState } from "react";
import {
  fetchIntakePlaybookLatest,
  fetchIntakePlaybookRecent,
  postIntakePlaybookPromote,
  postIntakePlaybookPromoteBulk,
  type IntakePlaybookCandidateApiRow,
  type IntakePlaybookRunApiRow,
} from "../api/client";
import { HelpCrossLink } from "../help/HelpCrossLink";

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
  onOpenHelpTopic?: (topicId: string) => void;
};

export function IntakePlaybookPanel({ accessToken, canPromote, visible, onOpenHelpTopic }: Props) {
  const [playbookId, setPlaybookId] = useState<string>("market_mode");
  const [latest, setLatest] = useState<{
    run: IntakePlaybookRunApiRow | null;
    candidates: IntakePlaybookCandidateApiRow[];
  } | null>(null);
  const [recent, setRecent] = useState<IntakePlaybookRunApiRow[] | null>(null);
  const [err, setErr] = useState("");
  const [loading, setLoading] = useState(false);
  const [promoteId, setPromoteId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [bulkBusy, setBulkBusy] = useState(false);

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

  useEffect(() => {
    setSelected(new Set());
  }, [playbookId, latest?.run?.id]);

  const summaryText = useMemo(() => {
    if (!latest?.run) return "";
    try {
      return JSON.stringify(latest.run.summary_json, null, 2);
    } catch {
      return String(latest.run.summary_json);
    }
  }, [latest]);

  const unpromotedIds = useMemo(
    () => latest?.candidates.filter((c) => !c.merged_engine_symbol_id).map((c) => c.id) ?? [],
    [latest],
  );

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const onPromoteBulk = async (ids: string[]) => {
    if (!accessToken || !canPromote || ids.length === 0) return;
    setBulkBusy(true);
    setErr("");
    try {
      const out = await postIntakePlaybookPromoteBulk(accessToken, { candidate_ids: ids });
      if (out.errors.length > 0) {
        setErr(
          `${out.promoted.length} ok, ${out.errors.length} hata: ${out.errors.map((e) => `${e.candidate_id}: ${e.message}`).join("; ")}`,
        );
      }
      setSelected(new Set());
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBulkBusy(false);
    }
  };

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
        Worker <code className="mono">intake_playbook_engine</code> — tablolar{" "}
        <code className="mono">intake_playbook_runs</code> / <code className="mono">intake_playbook_candidates</code>. Açmak:{" "}
        <code className="mono">QTSS_INTAKE_PLAYBOOK_ENABLED=1</code> veya <code className="mono">system_config</code>{" "}
        <code className="mono">intake_playbook_loop_enabled</code>. Telegram vb.:{" "}
        <code className="mono">QTSS_INTAKE_PLAYBOOK_NOTIFY_ENABLED=1</code> + <code className="mono">notify_outbox</code> döngüsü.
        {onOpenHelpTopic ? (
          <>
            {" "}
            <HelpCrossLink topicId="intake-playbook" onOpen={onOpenHelpTopic} label="Yardım" />
          </>
        ) : null}
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

      {latest && latest.candidates.length > 0 && canPromote && unpromotedIds.length > 0 ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "0.35rem", marginTop: "0.45rem" }}>
          <button
            type="button"
            className="theme-toggle"
            style={{ fontSize: "0.7rem" }}
            disabled={bulkBusy || selected.size === 0}
            onClick={() => void onPromoteBulk(Array.from(selected))}
          >
            {bulkBusy ? "…" : `Seçilenleri ekle (${selected.size})`}
          </button>
          <button
            type="button"
            className="theme-toggle"
            style={{ fontSize: "0.7rem" }}
            disabled={bulkBusy}
            onClick={() => void onPromoteBulk(unpromotedIds.slice(0, 25))}
          >
            Tümünü ekle (max 25)
          </button>
        </div>
      ) : null}

      {latest && latest.candidates.length > 0 ? (
        <table className="nansen-api-table" style={{ marginTop: "0.5rem", fontSize: "0.72rem" }}>
          <thead>
            <tr>
              {canPromote ? <th /> : null}
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
                {canPromote ? (
                  <td>
                    {!c.merged_engine_symbol_id ? (
                      <input
                        type="checkbox"
                        className="nansen-api-table__check"
                        checked={selected.has(c.id)}
                        onChange={() => toggleSelect(c.id)}
                        aria-label={`select ${c.symbol}`}
                      />
                    ) : null}
                  </td>
                ) : null}
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
