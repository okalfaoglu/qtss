import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchTelegramSetupAnalysisStatus, type TelegramSetupAnalysisStatusApi } from "../api/client";

type Props = {
  accessToken: string;
};

export function TelegramSetupAnalysisPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<TelegramSetupAnalysisStatusApi | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setErr("");
    setBusy(true);
    try {
      const s = await fetchTelegramSetupAnalysisStatus(accessToken);
      setStatus(s);
    } catch (e) {
      setErr(String(e));
      setStatus(null);
    } finally {
      setBusy(false);
    }
  }, [accessToken]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="card">
      <p className="tv-drawer__section-head">{t("telegramSetupAnalysis.heading")}</p>
      <p className="muted" style={{ marginTop: 0, fontSize: "0.72rem", lineHeight: 1.45 }}>
        {t("telegramSetupAnalysis.intro")}
      </p>

      <button type="button" className="tv-btn tv-btn--secondary" disabled={busy} onClick={() => void load()}>
        {busy ? t("telegramSetupAnalysis.refreshing") : t("telegramSetupAnalysis.refresh")}
      </button>

      {err ? (
        <p className="tv-drawer__error" style={{ marginTop: "0.75rem" }}>
          {err}
        </p>
      ) : null}

      {status ? (
        <dl className="tv-registry__dl" style={{ marginTop: "1rem" }}>
          <dt>{t("telegramSetupAnalysis.triggerPhrase")}</dt>
          <dd>
            <code>{status.trigger_phrase}</code>
          </dd>
          <dt>{t("telegramSetupAnalysis.webhookConfigured")}</dt>
          <dd>{status.webhook_configured ? t("telegramSetupAnalysis.statusYes") : t("telegramSetupAnalysis.statusNo")}</dd>
          <dt>{t("telegramSetupAnalysis.geminiConfigured")}</dt>
          <dd>{status.gemini_configured ? t("telegramSetupAnalysis.statusYes") : t("telegramSetupAnalysis.statusNo")}</dd>
          <dt>{t("telegramSetupAnalysis.geminiModel")}</dt>
          <dd>
            <code>{status.gemini_model}</code>
          </dd>
          <dt>{t("telegramSetupAnalysis.buffer")}</dt>
          <dd>
            {t("telegramSetupAnalysis.bufferValue", {
              max: status.max_buffer_turns,
              ttlSec: status.buffer_ttl_secs,
            })}
          </dd>
          <dt>{t("telegramSetupAnalysis.allowlist")}</dt>
          <dd>
            {status.allowlist_restricts
              ? t("telegramSetupAnalysis.allowlistOn", { count: status.allowlist_size })
              : t("telegramSetupAnalysis.allowlistOff")}
          </dd>
          <dt>{t("telegramSetupAnalysis.webhookPath")}</dt>
          <dd>
            <code>{status.webhook_path}</code>
          </dd>
        </dl>
      ) : null}

      <div style={{ marginTop: "1.25rem" }}>
        <p className="tv-registry__hint-title">{t("telegramSetupAnalysis.configTitle")}</p>
        <ul className="tv-registry__hint-list">
          <li>{t("telegramSetupAnalysis.hintModule")}</li>
          <li>{t("telegramSetupAnalysis.hintWebhook")}</li>
          <li>{t("telegramSetupAnalysis.hintGemini")}</li>
          <li>{t("telegramSetupAnalysis.hintNotify")}</li>
          <li>{t("telegramSetupAnalysis.hintFlow")}</li>
        </ul>
      </div>
    </div>
  );
}
