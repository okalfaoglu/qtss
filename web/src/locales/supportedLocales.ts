/**
 * Single source of truth for UI locale codes (FAZ 9).
 * Add a row here when introducing a new `locales/<code>.json` bundle; keep in sync with API `locale.rs` / `session` (`en` | `tr`).
 */
export type LocaleTextDirection = "ltr" | "rtl";

export type SupportedLocaleMeta = {
  code: string;
  nativeName: string;
  dir: LocaleTextDirection;
};

export const SUPPORTED_LOCALES: SupportedLocaleMeta[] = [
  { code: "en", nativeName: "English", dir: "ltr" },
  { code: "tr", nativeName: "Türkçe", dir: "ltr" },
];

export const SUPPORTED_LOCALE_CODES: string[] = SUPPORTED_LOCALES.map((l) => l.code);
