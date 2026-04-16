import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchSetupRejections,
  fetchSetupRejectionSummary,
  type SetupRejectionApi,
  type SetupRejectionSummaryApi,
} from "../api/client";

/**
 * Faz 9.1.3 — Confluence Inspector.
 *
 * Surfaces `qtss_v2_setup_rejections` so operators can answer
 * "why didn't we trade X?" for every vetoed candidate. Two views:
 *
 *   * Summary card — rejection count per reason bucket over a sliding
 *     window (default 24h).
 *   * Recent list — latest rejections with venue/symbol/profile detail.
 */

type Props = {
  accessToken: string | null;
};

const WINDOW_CHOICES: Array<{ hours: number; labelKey: string }> = [
  { hours: 1, labelKey: "confluenceInspector.window.1h" },
  { hours: 6, labelKey: "confluenceInspector.window.6h" },
  { hours: 24, labelKey: "confluenceInspector.window.24h" },
  { hours: 24 * 7, labelKey: "confluenceInspector.window.7d" },
  { hours: 24 * 30, labelKey: "confluenceInspector.window.30d" },
];

function reasonBadgeClass(reason: string): string {
  if (reason.startsWith("gate_")) return "tv-badge tv-badge--gate";
  if (reason === "commission_gate") return "tv-badge tv-badge--commission";
  return "tv-badge tv-badge--alloc";
}

export function ConfluenceInspectorPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [windowHours, setWindowHours] = useState<number>(24);
  const [reasonFilter, setReasonFilter] = useState<string>("");
  const [symbolFilter, setSymbolFilter] = useState<string>("");
  const [rows, setRows] = useState<SetupRejectionApi[]>([]);
  const [summary, setSummary] = useState<SetupRejectionSummaryApi | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    setBusy(true);
    setErr("");
    try {
      const [list, sum] = await Promise.all([
        fetchSetupRejections(accessToken, {
          limit: 200,
          sinceHours: windowHours,
          reason: reasonFilter.trim() || undefined,
          symbol: symbolFilter.trim() || undefined,
        }),
        fetchSetupRejectionSummary(accessToken, { sinceHours: windowHours }),
      ]);
      setRows(list.entries);
      setSummary(sum);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, windowHours, reasonFilter, symbolFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!accessToken) {
    return <p className="muted">{t("confluenceInspector.loginPrompt")}</p>;
  }

  return (
    <div className="card" style={{ marginTop: "0.5rem" }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "0.5rem",
          marginBottom: "0.5rem",
        }}
      >
        <p className="tv-drawer__section-head" style={{ margin: 0 }}>
          {t("confluenceInspector.title")}
        </p>
        <div style={{ display: "flex", gap: "0.4rem", alignItems: "center", flexWrap: "wrap" }}>
          <select
            value={windowHours}
            onChange={(e) => setWindowHours(Number(e.target.value))}
            style={{ fontSize: "0.75rem" }}
          >
            {WINDOW_CHOICES.map((w) => (
              <option key={w.hours} value={w.hours}>
                {t(w.labelKey)}
              </option>
            ))}
          </select>
          <input
            type="text"
            placeholder={t("confluenceInspector.filter.symbol") ?? ""}
            value={symbolFilter}
            onChange={(e) => setSymbolFilter(e.target.value)}
            style={{ fontSize: "0.75rem", width: 120 }}
          />
          <input
            type="text"
            placeholder={t("confluenceInspector.filter.reason") ?? ""}
            value={reasonFilter}
            onChange={(e) => setReasonFilter(e.target.value)}
            style={{ fontSize: "0.75rem", width: 160 }}
          />
          <button
            type="button"
            disabled={busy}
            onClick={() => void refresh()}
            style={{ fontSize: "0.75rem" }}
          >
            {busy ? t("confluenceInspector.refreshing") : t("confluenceInspector.refresh")}
          </button>
        </div>
      </div>

      {err && (
        <p className="muted" style={{ color: "#ffb4ab", fontSize: "0.75rem" }}>
          {err}
        </p>
      )}

      {summary && (
        <div
          style={{
            display: "flex",
            gap: "0.35rem",
            flexWrap: "wrap",
            marginBottom: "0.6rem",
            fontSize: "0.72rem",
          }}
        >
          <span className="muted">
            {t("confluenceInspector.summary.total", { n: summary.total })}
          </span>
          {summary.by_reason.map((b) => (
            <button
              key={b.reason}
              type="button"
              className={reasonBadgeClass(b.reason)}
              onClick={() =>
                setReasonFilter(reasonFilter === b.reason ? "" : b.reason)
              }
              style={{
                fontSize: "0.7rem",
                padding: "2px 6px",
                border: reasonFilter === b.reason ? "1px solid #ffd54f" : "1px solid transparent",
                cursor: "pointer",
              }}
              title={t("confluenceInspector.summary.clickToFilter") ?? ""}
            >
              {t(`confluenceInspector.reason.${b.reason}`, { defaultValue: b.reason })}
              {": "}
              <strong>{b.n}</strong>
            </button>
          ))}
        </div>
      )}

      {rows.length === 0 ? (
        <p className="muted" style={{ fontSize: "0.75rem" }}>
          {t("confluenceInspector.empty")}
        </p>
      ) : (
        <div style={{ overflowX: "auto" }}>
          <table
            className="tv-tr-setup-table"
            style={{ fontSize: "0.72rem", width: "100%" }}
          >
            <thead>
              <tr>
                <th>{t("confluenceInspector.col.time")}</th>
                <th>{t("confluenceInspector.col.venue")}</th>
                <th>{t("confluenceInspector.col.symbol")}</th>
                <th>{t("confluenceInspector.col.tf")}</th>
                <th>{t("confluenceInspector.col.profile")}</th>
                <th>{t("confluenceInspector.col.direction")}</th>
                <th>{t("confluenceInspector.col.reason")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td className="tv-tr-setup-mono">
                    {new Date(r.created_at).toLocaleString()}
                  </td>
                  <td>{r.venue_class}</td>
                  <td className="tv-tr-setup-mono">{r.symbol}</td>
                  <td>{r.timeframe}</td>
                  <td>{r.profile.toUpperCase()}</td>
                  <td>{r.direction}</td>
                  <td>
                    <span className={reasonBadgeClass(r.reject_reason)}>
                      {t(`confluenceInspector.reason.${r.reject_reason}`, {
                        defaultValue: r.reject_reason,
                      })}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
