import { useTranslation } from "react-i18next";
import { patchMePreferredLocale } from "../api/client";
import { SUPPORTED_LOCALES } from "../locales/supportedLocales";

type Props = {
  accessToken: string | null;
  onLocalePatched?: (code: string | null) => void;
};

export function LanguageSwitcher({ accessToken, onLocalePatched }: Props) {
  const { i18n, t } = useTranslation();
  const v = i18n.language.startsWith("tr") ? "tr" : "en";
  const textDir = SUPPORTED_LOCALES.find((l) => l.code === v)?.dir ?? "ltr";

  return (
    <label style={{ display: "inline-flex", gap: "0.35rem", alignItems: "center", fontSize: "0.75rem" }}>
      <span>{t("common.language")}</span>
      <select
        dir={textDir}
        value={v}
        onChange={(e) => {
          const next = e.target.value;
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
        {SUPPORTED_LOCALES.map((l) => (
          <option key={l.code} value={l.code} lang={l.code}>
            {l.nativeName}
          </option>
        ))}
      </select>
    </label>
  );
}
