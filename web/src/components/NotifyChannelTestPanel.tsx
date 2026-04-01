import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { postNotifyTest } from "../api/client";

type Props = {
  accessToken: string;
};

const NOTIFY_CHANNELS = [
  "telegram",
  "email",
  "sms",
  "whatsapp",
  "x",
  "facebook",
  "instagram",
  "discord",
  "webhook",
] as const;

type ChannelId = (typeof NOTIFY_CHANNELS)[number];

function channelLabelKey(id: ChannelId): string {
  return `notifyTest.channel.${id}`;
}

export function NotifyChannelTestPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [channel, setChannel] = useState<ChannelId>("telegram");
  const [title, setTitle] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [receipt, setReceipt] = useState<unknown>(null);

  const send = useCallback(async () => {
    setErr("");
    setReceipt(null);
    setBusy(true);
    try {
      const res = await postNotifyTest(accessToken, {
        channel,
        title: title.trim() || undefined,
        message: message.trim() || undefined,
      });
      setReceipt(res);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, channel, title, message]);

  return (
    <div className="card tv-notify-test">
      <p className="tv-drawer__section-head">{t("notifyTest.heading")}</p>
      <p className="muted" style={{ marginTop: 0, fontSize: "0.72rem", lineHeight: 1.45 }}>
        {t("notifyTest.hint")}
      </p>

      <div className="tv-notify-test__channel-tabs" role="tablist" aria-label={t("notifyTest.channelTabsAria")}>
        {NOTIFY_CHANNELS.map((id) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={channel === id}
            className={`tv-notify-test__channel-tab ${channel === id ? "is-active" : ""}`}
            onClick={() => {
              setChannel(id);
              setErr("");
              setReceipt(null);
            }}
          >
            {t(channelLabelKey(id))}
          </button>
        ))}
      </div>

      <div className="tv-notify-test__form">
        <label>
          <span className="muted">{t("notifyTest.fieldTitle")}</span>
          <input
            className="mono"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={t("notifyTest.titlePlaceholder")}
          />
        </label>
        <label>
          <span className="muted">{t("notifyTest.fieldBody")}</span>
          <textarea
            className="mono tv-notify-test__textarea"
            rows={3}
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder={t("notifyTest.bodyPlaceholder")}
            spellCheck={false}
          />
        </label>
        <button type="button" className="theme-toggle" disabled={busy} onClick={() => void send()}>
          {busy ? t("notifyTest.sending") : t("notifyTest.send")}
        </button>
      </div>

      {err ? (
        <pre className="mono err" style={{ fontSize: "0.72rem", marginTop: "0.5rem", whiteSpace: "pre-wrap" }}>
          {err}
        </pre>
      ) : null}
      {receipt != null ? (
        <div style={{ marginTop: "0.5rem" }}>
          <p className="muted" style={{ fontSize: "0.7rem", margin: "0 0 0.25rem" }}>
            {t("notifyTest.response")}
          </p>
          <pre className="mono" style={{ fontSize: "0.68rem", maxHeight: "10rem", overflow: "auto", margin: 0 }}>
            {JSON.stringify(receipt, null, 2)}
          </pre>
        </div>
      ) : null}
    </div>
  );
}
