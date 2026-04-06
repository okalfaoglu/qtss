import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { NotifyOutboxCard } from "./NotifyOutboxCard";
import { NotifyChannelTestPanel } from "./NotifyChannelTestPanel";
import { NotifyOutboxTemplatePanel } from "./NotifyOutboxTemplatePanel";

type Props = {
  accessToken: string;
  canOps: boolean;
};

type HubTab = "outbox" | "template" | "channel_test";

export function NotificationDrawerPanel({ accessToken, canOps }: Props) {
  const { t } = useTranslation();
  const [hubTab, setHubTab] = useState<HubTab>("outbox");
  const [outboxRefreshSignal, setOutboxRefreshSignal] = useState(0);

  const onTemplateEnqueued = useCallback(() => {
    setOutboxRefreshSignal((n) => n + 1);
  }, []);

  return (
    <div className="tv-notify-hub">
      <div className="tv-notify-hub__pickers" role="tablist" aria-label={t("notifyTest.hubAria")}>
        <button
          type="button"
          role="tab"
          aria-selected={hubTab === "outbox"}
          className={`tv-notify-hub__picker ${hubTab === "outbox" ? "is-active" : ""}`}
          onClick={() => setHubTab("outbox")}
        >
          <span className="tv-notify-hub__picker-icon" aria-hidden>
            ≡
          </span>
          <span className="tv-notify-hub__picker-text">
            <span className="tv-notify-hub__picker-label">{t("notifyTest.hubOutbox")}</span>
            <span className="tv-notify-hub__picker-hint">{t("notifyTest.hubOutboxDesc")}</span>
          </span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={hubTab === "template"}
          className={`tv-notify-hub__picker ${hubTab === "template" ? "is-active" : ""}`}
          onClick={() => setHubTab("template")}
        >
          <span className="tv-notify-hub__picker-icon" aria-hidden>
            ✎
          </span>
          <span className="tv-notify-hub__picker-text">
            <span className="tv-notify-hub__picker-label">{t("notifyTest.hubTemplate")}</span>
            <span className="tv-notify-hub__picker-hint">{t("notifyTest.hubTemplateDesc")}</span>
          </span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={hubTab === "channel_test"}
          className={`tv-notify-hub__picker ${hubTab === "channel_test" ? "is-active" : ""}`}
          onClick={() => setHubTab("channel_test")}
        >
          <span className="tv-notify-hub__picker-icon" aria-hidden>
            ✧
          </span>
          <span className="tv-notify-hub__picker-text">
            <span className="tv-notify-hub__picker-label">{t("notifyTest.hubChannels")}</span>
            <span className="tv-notify-hub__picker-hint">{t("notifyTest.hubChannelsDesc")}</span>
          </span>
        </button>
      </div>

      {hubTab === "outbox" ? (
        <NotifyOutboxCard accessToken={accessToken} refreshSignal={outboxRefreshSignal} />
      ) : null}
      {hubTab === "template" ? (
        <NotifyOutboxTemplatePanel
          accessToken={accessToken}
          canOps={canOps}
          onEnqueued={onTemplateEnqueued}
        />
      ) : null}
      {hubTab === "channel_test" ? <NotifyChannelTestPanel accessToken={accessToken} /> : null}
    </div>
  );
}
