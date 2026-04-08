// Auth helpers for the v2 web shell.
//
// The API exposes a public OAuth client whose credentials can be fetched
// from /api/v1/bootstrap/web-oauth-client. We cache them in localStorage so
// that subsequent logins do not need to re-fetch.

const TOKEN_KEY = "qtss.v2.token";
const CLIENT_KEY = "qtss.v2.oauth_client";

interface OAuthClient {
  client_id: string;
  client_secret: string;
  suggested_login_email?: string;
}

interface TokenResponse {
  access_token: string;
  token_type: string;
  expires_in: number;
  refresh_token?: string;
  scope?: string;
}

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function isAuthenticated(): boolean {
  return getToken() !== null;
}

export function logout(): void {
  localStorage.removeItem(TOKEN_KEY);
}

async function fetchBootstrap(): Promise<OAuthClient> {
  const cached = localStorage.getItem(CLIENT_KEY);
  if (cached) {
    return JSON.parse(cached) as OAuthClient;
  }
  const res = await fetch("/api/v1/bootstrap/web-oauth-client");
  if (!res.ok) {
    throw new Error(`bootstrap failed: ${res.status}`);
  }
  const client = (await res.json()) as OAuthClient;
  localStorage.setItem(CLIENT_KEY, JSON.stringify(client));
  return client;
}

export async function login(email: string, password: string): Promise<void> {
  const client = await fetchBootstrap();
  const body = new URLSearchParams({
    grant_type: "password",
    client_id: client.client_id,
    client_secret: client.client_secret,
    username: email,
    password,
  });
  const res = await fetch("/oauth/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`login failed: ${res.status} ${text}`);
  }
  const token = (await res.json()) as TokenResponse;
  localStorage.setItem(TOKEN_KEY, token.access_token);
}

export async function getSuggestedEmail(): Promise<string | undefined> {
  try {
    const client = await fetchBootstrap();
    return client.suggested_login_email;
  } catch {
    return undefined;
  }
}
