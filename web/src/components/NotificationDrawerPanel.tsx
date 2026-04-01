import { useState } from "react";
import { useTranslation } from "react-i18next";
import { NotifyOutboxCard } from "./NotifyOutboxCard";
import { NotifyChannelTestPanel } from "./NotifyChannelTestPanel";

type Props = {
  accessToken: string;
};

type HubTab = "outbox" | "channel_test";

export function NotificationDrawerPanel({ accessToken }: Props) {
  const { t } = useTranslation();
  const [hubTab, setHubTab] = useState<HubTab>("outbox");

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

      {hubTab === "outbox" ? <NotifyOutboxCard accessToken={accessToken} /> : null}
      {hubTab === "channel_test" ? <NotifyChannelTestPanel accessToken={accessToken} /> : null}
    </div>
  );
}
