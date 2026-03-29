/** Sunucu `roles` tablosu anahtarları ile uyumlu (JWT `roles` claim). */
export const ROLE_ADMIN = "admin";
export const ROLE_TRADER = "trader";
export const ROLE_ANALYST = "analyst";
export const ROLE_VIEWER = "viewer";

/** `GET /api/v1/me` sonrası GUI oturum özeti (JWT payload özeti). */
export type AuthSession = {
  userId: string;
  orgId: string;
  roles: string[];
  /** JWT `permissions` claim (e.g. `qtss:read`, `qtss:ops`, `qtss:admin`) when issued by API. */
  permissions: string[];
  oauthClientId: string;
};

export function hasAnyRole(roles: readonly string[], ...allowed: string[]): boolean {
  return allowed.some((a) => roles.includes(a));
}

/** API `require_dashboard_roles` ile aynı küme. */
export function canUseDashboard(roles: readonly string[]): boolean {
  return hasAnyRole(roles, ROLE_ADMIN, ROLE_TRADER, ROLE_ANALYST, ROLE_VIEWER);
}

export function canAdmin(roles: readonly string[]): boolean {
  return roles.includes(ROLE_ADMIN);
}

/** Yazma / operasyon uçları (`require_ops_roles`). */
export function canOps(roles: readonly string[]): boolean {
  return hasAnyRole(roles, ROLE_ADMIN, ROLE_TRADER);
}
