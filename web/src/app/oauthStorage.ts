/** OAuth tokens persist across full reload; cleared on logout. */
export const ACCESS_TOKEN_STORAGE_KEY = "qtss_access_token";
export const ACCESS_TOKEN_EXP_MS_STORAGE_KEY = "qtss_access_token_exp_ms";
export const REFRESH_TOKEN_STORAGE_KEY = "qtss_refresh_token";

export function readStoredAccessToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const t = localStorage.getItem(ACCESS_TOKEN_STORAGE_KEY);
    return t != null && t.trim() !== "" ? t.trim() : null;
  } catch {
    return null;
  }
}

export function readStoredRefreshToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const t = localStorage.getItem(REFRESH_TOKEN_STORAGE_KEY);
    return t != null && t.trim() !== "" ? t.trim() : null;
  } catch {
    return null;
  }
}

export function readStoredAccessExpMs(): number | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(ACCESS_TOKEN_EXP_MS_STORAGE_KEY);
    const n = raw ? Number(raw) : NaN;
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}
