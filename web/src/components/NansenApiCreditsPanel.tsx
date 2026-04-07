import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { fetchAdminSystemConfig, upsertAdminSystemConfig, type SystemConfigRowApi } from "../api/client";
import {
  NANSEN_REFERENCE_ONLY_ROWS,
  NANSEN_WORKER_LOOP_ROWS,
  type NansenApiCatalogRow,
} from "../lib/nansenApiCreditsCatalog";

type Props = {
  accessToken: string | null;
  canAdmin: boolean;
  visible: boolean;
};

function enabledFromRowValue(v: unknown): boolean | undefined {
  if (v && typeof v === "object" && "enabled" in v) {
    const e = (v as { enabled?: unknown }).enabled;
    return typeof e === "boolean" ? e : undefined;
  }
  return undefined;
}

export function NansenApiCreditsPanel({ accessToken, canAdmin, visible }: Props) {
  const [workerRows, setWorkerRows] = useState<SystemConfigRowApi[] | null>(null);
  const [loadErr, setLoadErr] = useState("");
  const [detailRowId, setDetailRowId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!accessToken || !canAdmin) {
      setWorkerRows(null);
      return;
    }
    setLoadErr("");
    try {
      const rows = await fetchAdminSystemConfig(accessToken, { module: "worker", limit: 500 });
      setWorkerRows(rows);
    } catch (e) {
      setWorkerRows(null);
      setLoadErr(String(e));
    }
  }, [accessToken, canAdmin]);

  useEffect(() => {
    if (!visible || !accessToken) return;
    void refresh();
  }, [visible, accessToken, refresh]);

  useEffect(() => {
    if (!visible) setDetailRowId(null);
  }, [visible]);

  const configByKey = useMemo(() => {
    const m = new Map<string, boolean | undefined>();
    if (!workerRows) return m;
    for (const r of workerRows) {
      m.set(r.config_key, enabledFromRowValue(r.value));
    }
    return m;
  }, [workerRows]);

  const effectiveEnabled = useCallback(
    (row: NansenApiCatalogRow): boolean => {
      if (!row.configKey) return false;
      const stored = configByKey.get(row.configKey);
      if (typeof stored === "boolean") return stored;
      if (row.kind === "worker_opt_in") return false;
      return true;
    },
    [configByKey],
  );

  const onToggle = (row: NansenApiCatalogRow, next: boolean) => {
    if (!accessToken || !canAdmin || !row.configKey) return;
    void upsertAdminSystemConfig(accessToken, {
      module: "worker",
      config_key: row.configKey,
      value: { enabled: next },
      description: `Nansen loop toggle (${row.envKey ?? row.path}).`,
      is_secret: false,
    })
      .then(() => refresh())
      .catch(() => {});
  };

  const showWorkerChecks = Boolean(canAdmin && accessToken);

  const renderTable = (title: string, rows: NansenApiCatalogRow[], showChecks: boolean) => {
    const colSpan = showChecks ? 4 : 3;
    return (
    <>
      <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
        {title}
      </p>
      <div className="nansen-api-table-wrap">
        <table className="nansen-api-table">
          <thead>
            <tr>
              {showChecks ? <th className="nansen-api-table__check" aria-label="Açık" /> : null}
              <th>API</th>
              <th className="nansen-api-table__credits">Kredi (çağrı)</th>
              <th className="nansen-api-table__info" aria-label="Amaç ve detay" />
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <Fragment key={row.id}>
                <tr>
                  {showChecks ? (
                    <td className="nansen-api-table__check">
                      {row.configKey ? (
                        <input
                          type="checkbox"
                          checked={effectiveEnabled(row)}
                          disabled={!showWorkerChecks}
                          onChange={(e) => onToggle(row, e.target.checked)}
                          title={row.envKey ? `Env: ${row.envKey}` : row.path}
                          aria-label={`${row.label} döngüsü`}
                        />
                      ) : (
                        <span className="muted" title="Worker döngüsü yok">
                          —
                        </span>
                      )}
                    </td>
                  ) : null}
                  <td>
                    <div className="nansen-api-table__label">{row.label}</div>
                    <div className="nansen-api-table__path mono muted">{row.path}</div>
                  </td>
                  <td className="nansen-api-table__credits">{row.creditsLabel}</td>
                  <td className="nansen-api-table__info">
                    <button
                      type="button"
                      className={`nansen-api-info-btn ${detailRowId === row.id ? "is-open" : ""}`}
                      aria-expanded={detailRowId === row.id}
                      aria-controls={`nansen-api-detail-${row.id}`}
                      id={`nansen-api-info-${row.id}`}
                      title="Bu API ne için kullanılır?"
                      onClick={() => setDetailRowId((id) => (id === row.id ? null : row.id))}
                    >
                      i
                    </button>
                  </td>
                </tr>
                {detailRowId === row.id ? (
                  <tr className="nansen-api-table__detail-row">
                    <td colSpan={colSpan} className="nansen-api-table__detail-cell">
                      <div
                        className="nansen-api-detail"
                        id={`nansen-api-detail-${row.id}`}
                        role="region"
                        aria-labelledby={`nansen-api-info-${row.id}`}
                      >
                        <div className="nansen-api-detail__head">
                          <strong className="nansen-api-detail__title">{row.label}</strong>
                          <button
                            type="button"
                            className="nansen-api-detail__close"
                            onClick={() => setDetailRowId(null)}
                          >
                            Kapat
                          </button>
                        </div>
                        <p className="nansen-api-detail__body">{row.purposeDetail}</p>
                      </div>
                    </td>
                  </tr>
                ) : null}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>
    </>
    );
  };

  return (
    <div className="card" style={{ marginTop: "0.65rem" }}>
      <p className="tv-drawer__section-head">Nansen API — kredi özeti ve döngü anahtarları</p>
      <p className="muted" style={{ fontSize: "0.74rem", marginTop: 0, lineHeight: 1.45 }}>
        Kredi değerleri Nansen dokümantasyonuna göre (planınıza göre değişebilir). İşaretli satırlar{" "}
        <code>system_config</code> üzerinden worker döngüsünü açar/kapatır; üstteki genel{" "}
        <strong>Nansen</strong> anahtarı da kapalıysa istek gitmez. Ortam değişkeni{" "}
        <code>QTSS_CONFIG_ENV_OVERRIDES=1</code> iken ilgili <code>NANSEN_*_ENABLED</code> değişkenleri önceliklidir.
      </p>
      {loadErr ? <p className="err" style={{ fontSize: "0.78rem" }}>{loadErr}</p> : null}
      {!showWorkerChecks ? (
        <p className="muted" style={{ fontSize: "0.72rem" }}>
          Döngü onay kutularını görmek / değiştirmek için yönetici olarak giriş yapın.
        </p>
      ) : null}
      {renderTable("Worker ile çalışan döngüler", NANSEN_WORKER_LOOP_ROWS, showWorkerChecks)}
      {renderTable("Henüz worker döngüsü olmayan API’ler (referans)", NANSEN_REFERENCE_ONLY_ROWS, false)}
    </div>
  );
}
