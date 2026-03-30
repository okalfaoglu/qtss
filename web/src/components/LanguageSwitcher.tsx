import { useTranslation } from "react-i18next";
import { patchMePreferredLocale } from "../api/client";

type Props = {
  accessToken: string | null;
  onLocalePatched?: (code: string | null) => void;
};

export function LanguageSwitcher({ accessToken, onLocalePatched }: Props) {
  const { i18n, t } = useTranslation();
  const v = i18n.language.startsWith("tr") ? "tr" : "en";

  return (
    <label style={{ display: "inline-flex", gap: "0.35rem", alignItems: "center", fontSize: "0.75rem" }}>
      <span>{t("common.language")}</span>
      <select
        value={v}
        onChange={(e) => {
          const next = e.target.value as "en" | "tr";
          void (async () => {
            await i18n.changeLanguage(next);
            if (accessToken) {
              try {
                await patchMePreferredLocale(accessToken, next);
                onLocalePatched?.(next);
              } catch {
                /* offline or RBAC — UI locale still switched */
              }
            }
          })();
        }}
        className="theme-toggle"
        aria-label={t("common.language")}
      >
        <option value="en">English</option>
        <option value="tr">Türkçe</option>
      </select>
    </label>
  );
}
