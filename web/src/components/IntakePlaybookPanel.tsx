import { useCallback, useEffect, useMemo, useState } from "react";
import {
  fetchIntakePlaybookLatest,
  fetchIntakePlaybookRecent,
  fetchLifecycleSummary,
  postIntakePlaybookPromote,
  postIntakePlaybookPromoteBulk,
  type IntakePlaybookCandidateApiRow,
  type IntakePlaybookRunApiRow,
  type LifecycleSummaryApiResponse,
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
  const [lifecycleSummary, setLifecycleSummary] = useState<LifecycleSummaryApiResponse | null>(null);

  const refresh = useCallback(async () => {
    if (!accessToken) {
      setLatest(null);
      setRecent(null);
      setLifecycleSummary(null);
      return;
    }
    setErr("");
    setLoading(true);
    try {
      const [lat, rec, lcs] = await Promise.all([
        fetchIntakePlaybookLatest(accessToken, playbookId),
        fetchIntakePlaybookRecent(accessToken, 25),
        fetchLifecycleSummary(accessToken).catch(() => null),
      ]);
      setLatest(lat);
      setRecent(rec);
      setLifecycleSummary(lcs);
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
        <code className="mono">intake_playbook_runs</code> / <code className="mono">intake_playbook_candidates</code>. Aç/kapa ve süre:{" "}
        <code className="mono">system_config</code> modül <code className="mono">worker</code> —{" "}
        <code className="mono">intake_playbook_loop_enabled</code>, <code className="mono">intake_playbook_tick_secs</code>. Bildirim:{" "}
        <code className="mono">intake_playbook_notify_enabled</code>, <code className="mono">intake_playbook_notify_channels</code> +{" "}
        <code className="mono">notify_outbox</code> döngüsü (<code className="mono">.env</code> yalnız{" "}
        <code className="mono">QTSS_CONFIG_ENV_OVERRIDES=1</code> ile acil üzerine yazma).
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
        <p className="muted" style={{ fontSize: "0.75rem", lineHeight: 1.45 }}>
          Bu playbook için henüz run yok. Kontrol listesi: (1) Admin <code className="mono">system_config</code>{" "}
          <code className="mono">worker.intake_playbook_loop_enabled</code> → <code className="mono">{"{ \"enabled\": true }"}</code>; (2){" "}
          <code className="mono">data_snapshots</code> dolu — özellikle{" "}
          <code className="mono">nansen_token_screener</code> ve ilgili Nansen/Binance worker döngüleri + <code className="mono">NANSEN_API_KEY</code>
          ; (3) çıplak kurulumda migration <code className="mono">0003</code>–<code className="mono">0007</code> uygulanmış olsun (tablolar{" "}
          <code className="mono">0001_qtss_baseline</code> ile de gelir). API:{" "}
          <code className="mono">GET …/analysis/intake-playbook/recent?limit=5</code> (JWT) — 200 ve <code>[]</code> ise motor henüz yazmamıştır.
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

      {lifecycleSummary ? (
        <>
          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
            Lifecycle summary
          </p>
          <div style={{ display: "flex", gap: "0.35rem", flexWrap: "wrap", marginTop: "0.25rem" }}>
            {Object.entries(lifecycleSummary.lifecycle_summary).map(([state, count]) => (
              <LifecycleBadge key={state} state={state} count={count} />
            ))}
          </div>
        </>
      ) : null}
    </div>
  );
}

const LIFECYCLE_BADGE_COLORS: Record<string, { bg: string; fg: string }> = {
  promoted: { bg: "#3b82f6", fg: "#fff" },
  analyzing: { bg: "#eab308", fg: "#000" },
  ready: { bg: "#22c55e", fg: "#fff" },
  trading: { bg: "#f97316", fg: "#fff" },
  closing: { bg: "#ef4444", fg: "#fff" },
  cooldown: { bg: "#6b7280", fg: "#fff" },
  retired: { bg: "#374151", fg: "#9ca3af" },
  manual: { bg: "#1f2937", fg: "#6b7280" },
};

function LifecycleBadge({ state, count }: { state: string; count: number }) {
  const colors = LIFECYCLE_BADGE_COLORS[state] ?? { bg: "#374151", fg: "#d1d5db" };
  return (
    <span
      style={{
        display: "inline-block",
        fontSize: "0.62rem",
        fontFamily: "monospace",
        padding: "0.12rem 0.4rem",
        borderRadius: "0.25rem",
        backgroundColor: colors.bg,
        color: colors.fg,
        lineHeight: 1.5,
      }}
    >
      {state}: {count}
    </span>
  );
}
