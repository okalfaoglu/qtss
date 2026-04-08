// Tiny typed fetch wrapper. All v2 endpoints live under /api/v1/v2/* and
// require a Bearer token from the OAuth password grant flow.

import { getToken, logout } from "./auth";

const API_BASE = "/api/v1";

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers = new Headers(init.headers);
  headers.set("accept", "application/json");
  if (token) {
    headers.set("authorization", `Bearer ${token}`);
  }
  const res = await fetch(`${API_BASE}${path}`, { ...init, headers });
  if (res.status === 401) {
    // Token expired or revoked — clear it so the router bounces to /login.
    logout();
    throw new ApiError(401, "unauthorized");
  }
  if (!res.ok) {
    const text = await res.text();
    throw new ApiError(res.status, text || res.statusText);
  }
  return (await res.json()) as T;
}
