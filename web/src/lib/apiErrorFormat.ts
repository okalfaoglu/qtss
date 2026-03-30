/**
 * QTSS JSON API errors: `error`, optional `error_key`, `error_args` (FAZ 9.2).
 * When `error_key` matches `apiErrors.<key>` in locales, the message is translated.
 */

import i18n from "../i18n";

export type QtssApiErrorBody = {
  error?: string;
  /** OAuth token endpoint (`error_description` preferred over `error` for humans). */
  error_description?: string;
  locale?: string;
  error_key?: string;
  error_args?: Record<string, unknown>;
};

const MAX_DETAIL = 900;

export function parseQtssApiErrorJson(text: string): QtssApiErrorBody | null {
  try {
    const j = JSON.parse(text) as unknown;
    if (!j || typeof j !== "object") return null;
    const o = j as Record<string, unknown>;
    if (typeof o.error !== "string" && typeof o.error_key !== "string") return null;
    const argsRaw = o.error_args;
    let error_args: Record<string, unknown> | undefined;
    if (argsRaw !== null && typeof argsRaw === "object" && !Array.isArray(argsRaw)) {
      error_args = argsRaw as Record<string, unknown>;
    }
    return {
      error: typeof o.error === "string" ? o.error : undefined,
      locale: typeof o.locale === "string" ? o.locale : undefined,
      error_key: typeof o.error_key === "string" ? o.error_key : undefined,
      error_args,
    };
  } catch {
    return null;
  }
}

function i18nInterpolationFromArgs(args: Record<string, unknown> | undefined): Record<string, string | number> {
  if (!args) return {};
  const out: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(args)) {
    if (typeof v === "string" || typeof v === "number") out[k] = v;
    else if (typeof v === "boolean") out[k] = v ? 1 : 0;
    else if (v != null) out[k] = JSON.stringify(v);
  }
  return out;
}

/**
 * `status: detail` where detail prefers i18n for `error_key`, else server `error` or raw body (truncated).
 */
export function formatQtssApiErrorMessage(status: number, text: string): string {
  const slice = text.length > MAX_DETAIL ? `${text.slice(0, MAX_DETAIL)}…` : text;
  const body = parseQtssApiErrorJson(text);
  const fallback = (body?.error?.trim() || slice.trim() || "(no body)").trim();
  const key = body?.error_key?.trim();
  if (!key) {
    return `${status}: ${fallback}`;
  }
  const interpolation = i18nInterpolationFromArgs(body.error_args);
  const translated = String(
    i18n.t(`apiErrors.${key}`, {
      ...interpolation,
      defaultValue: fallback,
    }),
  );
  return `${status}: ${translated}`;
}

export function throwQtssApiError(endpointLabel: string, res: Response, bodyText: string): never {
  throw new Error(`${endpointLabel} ${formatQtssApiErrorMessage(res.status, bodyText)}`);
}
