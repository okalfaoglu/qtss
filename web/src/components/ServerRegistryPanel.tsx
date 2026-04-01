import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  deleteAdminSystemConfig,
  deleteAppConfig,
  fetchAdminSystemConfig,
  fetchAdminSystemConfigRow,
  fetchConfigList,
  upsertAdminSystemConfig,
  upsertAppConfig,
  type AppConfigEntryApi,
  type SystemConfigRowApi,
} from "../api/client";

type Props = {
  accessToken: string;
};

type RegistrySubtab = "system" | "application";

function valuePreview(v: unknown, max = 96): string {
  try {
    const s = JSON.stringify(v);
    if (s.length <= max) return s;
    return `${s.slice(0, max)}…`;
  } catch {
    return String(v);
  }
}

function isMaskedSystemValue(v: unknown): boolean {
  return Boolean(v && typeof v === "object" && "_masked" in (v as object));
}

export function ServerRegistryPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [subtab, setSubtab] = useState<RegistrySubtab>("system");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [moduleFilter, setModuleFilter] = useState("");

  const [systemRows, setSystemRows] = useState<SystemConfigRowApi[]>([]);
  const [appRows, setAppRows] = useState<AppConfigEntryApi[]>([]);

  const [sysModule, setSysModule] = useState("notify");
  const [sysKey, setSysKey] = useState("");
  const [sysValueJson, setSysValueJson] = useState('{"value":""}');
  const [sysDesc, setSysDesc] = useState("");
  const [sysSecret, setSysSecret] = useState(false);
  const [sysEditing, setSysEditing] = useState(false);

  const [appKey, setAppKey] = useState("");
  const [appValueJson, setAppValueJson] = useState("{}");
  const [appDesc, setAppDesc] = useState("");
  const [appEditing, setAppEditing] = useState(false);

  const refreshSystem = useCallback(async () => {
    setErr("");
    const rows = await fetchAdminSystemConfig(accessToken, {
      module: moduleFilter.trim() || undefined,
      limit: 500,
    });
    setSystemRows(rows);
  }, [accessToken, moduleFilter]);

  const refreshApp = useCallback(async () => {
    setErr("");
    const rows = await fetchConfigList(accessToken);
    setAppRows(rows);
  }, [accessToken]);

  useEffect(() => {
    if (subtab === "system") void refreshSystem().catch((e) => setErr(String(e)));
  }, [subtab, refreshSystem]);

  useEffect(() => {
    if (subtab === "application") void refreshApp().catch((e) => setErr(String(e)));
  }, [subtab, refreshApp]);

  const resetSystemForm = () => {
    setSysModule("notify");
    setSysKey("");
    setSysValueJson('{"value":""}');
    setSysDesc("");
    setSysSecret(false);
    setSysEditing(false);
  };

  const resetAppForm = () => {
    setAppKey("");
    setAppValueJson("{}");
    setAppDesc("");
    setAppEditing(false);
  };

  const onEditSystem = async (row: SystemConfigRowApi) => {
    setErr("");
    setBusy(true);
    try {
      const full = await fetchAdminSystemConfigRow(accessToken, row.module, row.config_key);
      setSysModule(full.module);
      setSysKey(full.config_key);
      setSysValueJson(JSON.stringify(full.value, null, 2));
      setSysDesc(full.description ?? "");
      setSysSecret(full.is_secret);
      setSysEditing(true);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onSaveSystem = async () => {
    setErr("");
    let parsed: unknown;
    try {
      parsed = JSON.parse(sysValueJson);
    } catch {
      setErr(t("settingsRegistry.invalidJson"));
      return;
    }
    setBusy(true);
    try {
      await upsertAdminSystemConfig(accessToken, {
        module: sysModule.trim(),
        config_key: sysKey.trim(),
        value: parsed,
        description: sysDesc.trim() || undefined,
        is_secret: sysSecret,
      });
      resetSystemForm();
      await refreshSystem();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDeleteSystem = async (row: SystemConfigRowApi) => {
    if (
      !window.confirm(t("settingsRegistry.confirmDeleteSystem", { key: `${row.module}.${row.config_key}` }))
    ) {
      return;
    }
    setErr("");
    setBusy(true);
    try {
      await deleteAdminSystemConfig(accessToken, row.module, row.config_key);
      if (sysEditing && sysModule === row.module && sysKey === row.config_key) resetSystemForm();
      await refreshSystem();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onEditApp = (row: AppConfigEntryApi) => {
    setAppKey(row.key);
    setAppValueJson(JSON.stringify(row.value, null, 2));
    setAppDesc(row.description ?? "");
    setAppEditing(true);
  };

  const onSaveApp = async () => {
    setErr("");
    let parsed: unknown;
    try {
      parsed = JSON.parse(appValueJson);
    } catch {
      setErr(t("settingsRegistry.invalidJson"));
      return;
    }
    setBusy(true);
    try {
      await upsertAppConfig(accessToken, {
        key: appKey.trim(),
        value: parsed,
        description: appDesc.trim() || undefined,
      });
      resetAppForm();
      await refreshApp();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDeleteApp = async (row: AppConfigEntryApi) => {
    if (!window.confirm(t("settingsRegistry.confirmDeleteApp", { key: row.key }))) return;
    setErr("");
    setBusy(true);
    try {
      await deleteAppConfig(accessToken, row.key);
      if (appEditing && appKey === row.key) resetAppForm();
      await refreshApp();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="tv-registry card">
      <div className="tv-registry__header">
        <div>
          <p className="tv-registry__title">{t("settingsRegistry.title")}</p>
          <p className="tv-registry__subtitle muted">{t("settingsRegistry.subtitle")}</p>
        </div>
      </div>

      <div className="tv-registry__pickers" role="tablist" aria-label={t("settingsRegistry.subtabAria")}>
        <button
          type="button"
          role="tab"
          aria-selected={subtab === "system"}
          className={`tv-registry__picker ${subtab === "system" ? "is-active" : ""}`}
          onClick={() => {
            setSubtab("system");
            setErr("");
          }}
        >
          <span className="tv-registry__picker-icon" aria-hidden>
            ◆
          </span>
          <span className="tv-registry__picker-text">
            <span className="tv-registry__picker-label">{t("settingsRegistry.tabSystem")}</span>
            <span className="tv-registry__picker-hint">{t("settingsRegistry.tabSystemDesc")}</span>
          </span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={subtab === "application"}
          className={`tv-registry__picker ${subtab === "application" ? "is-active" : ""}`}
          onClick={() => {
            setSubtab("application");
            setErr("");
          }}
        >
          <span className="tv-registry__picker-icon" aria-hidden>
            ◇
          </span>
          <span className="tv-registry__picker-text">
            <span className="tv-registry__picker-label">{t("settingsRegistry.tabApp")}</span>
            <span className="tv-registry__picker-hint">{t("settingsRegistry.tabAppDesc")}</span>
          </span>
        </button>
      </div>

      {err ? (
        <p className="err" style={{ margin: "0.5rem 0 0", fontSize: "0.82rem" }}>
          {err}
        </p>
      ) : null}

      {subtab === "system" ? (
        <div className="tv-registry__panel">
          <aside className="tv-registry__hint">
            <p className="tv-registry__hint-title">{t("settingsRegistry.telegramHintTitle")}</p>
            <ul className="tv-registry__hint-list muted">
              <li>{t("settingsRegistry.telegramHintToken")}</li>
              <li>{t("settingsRegistry.telegramHintChat")}</li>
            </ul>
          </aside>

          <div className="tv-registry__toolbar">
            <label className="tv-registry__filter">
              <span className="muted">{t("settingsRegistry.moduleFilter")}</span>
              <input
                className="mono"
                value={moduleFilter}
                onChange={(e) => setModuleFilter(e.target.value)}
                placeholder="notify"
              />
            </label>
            <button
              type="button"
              className="theme-toggle"
              disabled={busy}
              onClick={() => void refreshSystem().catch((e) => setErr(String(e)))}
            >
              {t("settingsRegistry.refresh")}
            </button>
          </div>

          <div className="tv-registry__form">
            <p className="tv-drawer__section-head">{sysEditing ? t("settingsRegistry.editRow") : t("settingsRegistry.newRow")}</p>
            <div className="tv-registry__form-grid">
              <label>
                <span className="muted">{t("settingsRegistry.colModule")}</span>
                <input className="mono" value={sysModule} onChange={(e) => setSysModule(e.target.value)} disabled={sysEditing} />
              </label>
              <label>
                <span className="muted">{t("settingsRegistry.colKey")}</span>
                <input className="mono" value={sysKey} onChange={(e) => setSysKey(e.target.value)} disabled={sysEditing} />
              </label>
              <label className="tv-registry__check">
                <input type="checkbox" checked={sysSecret} onChange={(e) => setSysSecret(e.target.checked)} />
                <span>{t("settingsRegistry.secretFlag")}</span>
              </label>
            </div>
            <label>
              <span className="muted">{t("settingsRegistry.valueJson")}</span>
              <textarea
                className="mono tv-registry__textarea"
                rows={6}
                value={sysValueJson}
                onChange={(e) => setSysValueJson(e.target.value)}
                spellCheck={false}
              />
            </label>
            <label>
              <span className="muted">{t("settingsRegistry.description")}</span>
              <input value={sysDesc} onChange={(e) => setSysDesc(e.target.value)} />
            </label>
            <div className="tv-registry__actions">
              <button type="button" className="theme-toggle" disabled={busy || !sysModule.trim() || !sysKey.trim()} onClick={() => void onSaveSystem()}>
                {t("settingsRegistry.save")}
              </button>
              {sysEditing ? (
                <button type="button" className="theme-toggle" disabled={busy} onClick={resetSystemForm}>
                  {t("settingsRegistry.cancel")}
                </button>
              ) : null}
            </div>
          </div>

          <div className="tv-registry__table-wrap">
            <table className="tv-registry__table">
              <thead>
                <tr>
                  <th>{t("settingsRegistry.colModule")}</th>
                  <th>{t("settingsRegistry.colKey")}</th>
                  <th>{t("settingsRegistry.colValue")}</th>
                  <th>{t("settingsRegistry.colSecret")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {systemRows.map((row) => (
                  <tr key={row.id}>
                    <td className="mono">{row.module}</td>
                    <td className="mono">{row.config_key}</td>
                    <td className="mono tv-registry__cell-preview" title={valuePreview(row.value, 4000)}>
                      {isMaskedSystemValue(row.value) ? t("settingsRegistry.masked") : valuePreview(row.value)}
                    </td>
                    <td>{row.is_secret ? "•" : "—"}</td>
                    <td className="tv-registry__row-actions">
                      <button type="button" className="theme-toggle" disabled={busy} onClick={() => void onEditSystem(row)}>
                        {t("settingsRegistry.edit")}
                      </button>
                      <button type="button" className="theme-toggle" disabled={busy} onClick={() => void onDeleteSystem(row)}>
                        {t("settingsRegistry.delete")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {systemRows.length === 0 ? <p className="muted">{t("settingsRegistry.emptySystem")}</p> : null}
          </div>
        </div>
      ) : (
        <div className="tv-registry__panel">
          <div className="tv-registry__toolbar">
            <button
              type="button"
              className="theme-toggle"
              disabled={busy}
              onClick={() => void refreshApp().catch((e) => setErr(String(e)))}
            >
              {t("settingsRegistry.refresh")}
            </button>
          </div>

          <div className="tv-registry__form">
            <p className="tv-drawer__section-head">{appEditing ? t("settingsRegistry.editRow") : t("settingsRegistry.newRow")}</p>
            <label>
              <span className="muted">{t("settingsRegistry.appConfigKey")}</span>
              <input className="mono" value={appKey} onChange={(e) => setAppKey(e.target.value)} disabled={appEditing} />
            </label>
            <label>
              <span className="muted">{t("settingsRegistry.valueJson")}</span>
              <textarea
                className="mono tv-registry__textarea"
                rows={8}
                value={appValueJson}
                onChange={(e) => setAppValueJson(e.target.value)}
                spellCheck={false}
              />
            </label>
            <label>
              <span className="muted">{t("settingsRegistry.description")}</span>
              <input value={appDesc} onChange={(e) => setAppDesc(e.target.value)} />
            </label>
            <div className="tv-registry__actions">
              <button type="button" className="theme-toggle" disabled={busy || !appKey.trim()} onClick={() => void onSaveApp()}>
                {t("settingsRegistry.save")}
              </button>
              {appEditing ? (
                <button type="button" className="theme-toggle" disabled={busy} onClick={resetAppForm}>
                  {t("settingsRegistry.cancel")}
                </button>
              ) : null}
            </div>
          </div>

          <div className="tv-registry__table-wrap">
            <table className="tv-registry__table">
              <thead>
                <tr>
                  <th>{t("settingsRegistry.appConfigKey")}</th>
                  <th>{t("settingsRegistry.colValue")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {appRows.map((row) => (
                  <tr key={row.id}>
                    <td className="mono">{row.key}</td>
                    <td className="mono tv-registry__cell-preview" title={valuePreview(row.value, 4000)}>
                      {valuePreview(row.value)}
                    </td>
                    <td className="tv-registry__row-actions">
                      <button type="button" className="theme-toggle" disabled={busy} onClick={() => onEditApp(row)}>
                        {t("settingsRegistry.edit")}
                      </button>
                      <button type="button" className="theme-toggle" disabled={busy} onClick={() => void onDeleteApp(row)}>
                        {t("settingsRegistry.delete")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {appRows.length === 0 ? <p className="muted">{t("settingsRegistry.emptyApp")}</p> : null}
          </div>
        </div>
      )}
    </div>
  );
}
