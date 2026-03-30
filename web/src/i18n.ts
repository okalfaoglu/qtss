import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import tr from "./locales/tr.json";
import { SUPPORTED_LOCALE_CODES, SUPPORTED_LOCALES } from "./locales/supportedLocales";

const translationByCode: Record<string, object> = { en, tr };
for (const l of SUPPORTED_LOCALES) {
  if (translationByCode[l.code] === undefined) {
    throw new Error(`i18n: add locales/${l.code}.json and import it in i18n.ts`);
  }
}

const resources = Object.fromEntries(
  SUPPORTED_LOCALES.map((l) => [l.code, { translation: translationByCode[l.code] }]),
);

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: "en",
    supportedLngs: SUPPORTED_LOCALE_CODES,
    nonExplicitSupportedLngs: true,
    interpolation: { escapeValue: false },
    detection: {
      order: ["localStorage", "navigator"],
      caches: ["localStorage"],
      lookupLocalStorage: "qtss_i18nextLng",
    },
  });

export default i18n;
