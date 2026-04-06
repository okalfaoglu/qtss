import { useState } from "react";
import { useTranslation } from "react-i18next";
import { postNotifyOutbox } from "../api/client";

type Props = {
  accessToken: string;
  canOps: boolean;
  /** Called after a successful enqueue so the Outbox tab can refresh if needed. */
  onEnqueued?: () => void;
};

export function NotifyOutboxTemplatePanel({ accessToken, canOps, onEnqueued }: Props) {
  const { t } = useTranslation();
  const [notifyTitle, setNotifyTitle] = useState("");
  const [notifyBody, setNotifyBody] = useState("");
  const [notifyChannels, setNotifyChannels] = useState("webhook");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const enqueueNotify = async () => {
    if (!canOps) return;
    setErr("");
    setBusy(true);
    try {
      const ch = notifyChannels
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      await postNotifyOutbox(accessToken, {
        title: notifyTitle.trim(),
        body: notifyBody.trim(),
        channels: ch.length ? ch : undefined,
      });
      setNotifyTitle("");
      setNotifyBody("");
      onEnqueued?.();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card">
      <p className="tv-drawer__section-head">{t("notifyTest.templateHeading")}</p>
      <p className="muted" style={{ marginTop: 0, fontSize: "0.82rem" }}>
        {t("notifyTest.templateIntro")}
      </p>
      {err ? <p className="err">{err}</p> : null}
      {canOps ? (
        <div style={{ marginTop: "0.65rem", display: "flex", flexDirection: "column", gap: "0.4rem" }}>
          <label>
            <span className="muted" style={{ fontSize: "0.72rem" }}>
              {t("notifyTest.templateFieldTitle")}
            </span>
            <input className="mono" value={notifyTitle} onChange={(e) => setNotifyTitle(e.target.value)} />
          </label>
          <label>
            <span className="muted" style={{ fontSize: "0.72rem" }}>
              {t("notifyTest.templateFieldBody")}
            </span>
            <textarea
              className="mono"
              rows={3}
              value={notifyBody}
              onChange={(e) => setNotifyBody(e.target.value)}
              style={{ width: "100%" }}
            />
          </label>
          <label>
            <span className="muted" style={{ fontSize: "0.72rem" }}>
              {t("notifyTest.templateFieldChannels")}
            </span>
            <input className="mono" value={notifyChannels} onChange={(e) => setNotifyChannels(e.target.value)} />
          </label>
          <button type="button" className="theme-toggle" disabled={busy} onClick={() => void enqueueNotify()}>
            {busy ? t("notifyTest.templateEnqueueing") : t("notifyTest.templateEnqueue")}
          </button>
        </div>
      ) : (
        <p className="muted" style={{ marginTop: "0.5rem", fontSize: "0.72rem" }}>
          {t("notifyTest.templateOpsOnly")}
        </p>
      )}
    </div>
  );
}
