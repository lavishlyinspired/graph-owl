import { createContext, useContext, useEffect, useState, useSyncExternalStore } from "react";
import { setTokenSource } from "../api";
import {
  authorizeUrl,
  base64UrlEncode,
  readCallback,
  rememberHandoff,
  stateMatches,
  takeHandoff,
} from "./pkce";

/** Build-time configuration, not literals.
 *
 *  A public SPA client id is not a secret, but a tenant baked into source is
 *  still a code change for every deployment that is not this one — and the
 *  fallbacks below are a development tenant, which is exactly the value that
 *  must not silently ship to production. */
const AUTH0_DOMAIN =
  import.meta.env.VITE_OIDC_DOMAIN ?? "dev-uzuxwkbcozynti2m.us.auth0.com";
const CLIENT_ID =
  import.meta.env.VITE_OIDC_CLIENT_ID ?? "1YagciKTz39X5IlvZnYjFylGgzJRVkI6";
const AUDIENCE = import.meta.env.VITE_OIDC_AUDIENCE ?? "https://graph-owl.dev/api";
const SCOPE = "openid profile email offline_access";
const REDIRECT_URI = `${window.location.origin}/callback`;

function randomVerifier(): string {
  const array = new Uint8Array(32);
  crypto.getRandomValues(array);
  return base64UrlEncode(array);
}

async function challengeFor(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return base64UrlEncode(new Uint8Array(digest));
}

export interface AuthState {
  status: "loading" | "authenticated" | "unauthenticated";
  error: string | null;
}

interface AuthContextValue {
  state: AuthState;
  login: () => void;
  logout: () => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

// ---- In-memory token storage ----

// Tokens stay here and nowhere else — `00f`'s rule, and the reason is that a
// bearer credential readable from storage is a bearer credential an XSS reads.
// The PKCE state and verifier deliberately do *not* live here: see `pkce.ts`.
let _accessToken: string | null = null;
let _refreshToken: string | null = null;
let _refreshInFlight: Promise<boolean> | null = null;

const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

export function getAccessToken(): string | null {
  return _accessToken;
}

// Wired at module scope, not on mount. A component effect runs after the first
// render, and the first render is what fires the catalog's opening requests —
// which would then go out unauthenticated exactly once, on every page load.
setTokenSource(getAccessToken);

export function subscribeToTokenChanges(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getTokenSnapshot(): string | null {
  return _accessToken;
}

// ---- PKCE flow ----

function startUrl(state: string, challenge: string): string {
  return authorizeUrl({
    domain: AUTH0_DOMAIN,
    clientId: CLIENT_ID,
    redirectUri: REDIRECT_URI,
    audience: AUDIENCE,
    scope: SCOPE,
    state,
    challenge,
  });
}

async function exchangeCode(code: string, codeVerifier: string): Promise<void> {
  const response = await fetch(`https://${AUTH0_DOMAIN}/oauth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      client_id: CLIENT_ID,
      code,
      redirect_uri: REDIRECT_URI,
      code_verifier: codeVerifier,
    }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`token exchange failed: ${response.status} ${text}`);
  }

  const data = await response.json();
  _accessToken = data.access_token ?? null;
  _refreshToken = data.refresh_token ?? null;
  notify();
}

async function refreshAccessToken(): Promise<boolean> {
  if (_refreshInFlight) return _refreshInFlight;

  _refreshInFlight = (async () => {
    const rt = _refreshToken;
    if (!rt) return false;

    try {
      const response = await fetch(`https://${AUTH0_DOMAIN}/oauth/token`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          grant_type: "refresh_token",
          client_id: CLIENT_ID,
          refresh_token: rt,
        }),
      });

      if (!response.ok) {
        _accessToken = null;
        _refreshToken = null;
        notify();
        return false;
      }

      const data = await response.json();
      _accessToken = data.access_token ?? null;
      if (data.refresh_token) _refreshToken = data.refresh_token;
      notify();
      return true;
    } catch {
      _accessToken = null;
      _refreshToken = null;
      notify();
      return false;
    } finally {
      _refreshInFlight = null;
    }
  })();

  return _refreshInFlight;
}

// ---- Auth provider ----

/** Whether this document was loaded by the provider redirecting back to us.
 *
 *  Read once, before React renders, so the first paint can be the "signing
 *  in" state rather than a flash of the sign-in screen the user just left. */
const arrivedOnCallback = readCallback(window.location.search);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const token = useSyncExternalStore(subscribeToTokenChanges, getTokenSnapshot);
  const [exchange, setExchange] = useState<{
    status: "idle" | "exchanging";
    error: string | null;
  }>(
    arrivedOnCallback.kind === "code"
      ? { status: "exchanging", error: null }
      : {
          status: "idle",
          error:
            arrivedOnCallback.kind === "error"
              ? (arrivedOnCallback.description ?? arrivedOnCallback.error)
              : null,
        },
  );

  // In an effect, not in render. Exchanging a code is a side effect with a
  // one-shot resource: React renders a component twice under StrictMode, and
  // an authorization code is single-use, so the second attempt fails and the
  // first sign-in of every session looks broken.
  useEffect(() => {
    if (arrivedOnCallback.kind === "none") return;

    // The query string goes before anything can fail. A code left in the URL
    // survives into the browser's history, the Referer header of the next
    // outbound request, and whatever the user pastes when reporting a problem.
    window.history.replaceState({}, "", window.location.origin);

    if (arrivedOnCallback.kind === "error") return;

    const { state, verifier } = takeHandoff(window.sessionStorage);
    if (!stateMatches(arrivedOnCallback.state, state ?? null) || verifier === undefined) {
      setExchange({
        status: "idle",
        error:
          "that sign-in could not be verified as one this browser started. " +
          "Please sign in again.",
      });
      return;
    }

    exchangeCode(arrivedOnCallback.code, verifier)
      .then(() => setExchange({ status: "idle", error: null }))
      .catch((error: unknown) =>
        setExchange({
          status: "idle",
          error: error instanceof Error ? error.message : "sign-in failed",
        }),
      );
  }, []);

  const authState: AuthState = token
    ? { status: "authenticated", error: null }
    : exchange.status === "exchanging"
      ? { status: "loading", error: null }
      : { status: "unauthenticated", error: exchange.error };

  const value: AuthContextValue = {
    state: authState,
    login: async () => {
      const state = randomVerifier();
      const verifier = randomVerifier();
      const challenge = await challengeFor(verifier);

      // Parked in `sessionStorage`, because the navigation below destroys this
      // JS context and a verifier held in memory is `null` by the time the
      // callback needs it. See `pkce.ts` for why that is not a weakening of
      // the in-memory token rule.
      rememberHandoff(window.sessionStorage, { state, verifier });

      window.location.href = startUrl(state, challenge);
    },
    logout: () => {
      _accessToken = null;
      _refreshToken = null;
      notify();

      const params = new URLSearchParams({
        client_id: CLIENT_ID,
        returnTo: window.location.origin,
      });
      window.location.href = `https://${AUTH0_DOMAIN}/v2/logout?${params}`;
    },
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}

// ---- API integration (called from api.ts) ----

let _onRefreshFailed: (() => void) | null = null;

export function setOnRefreshFailed(fn: () => void) {
  _onRefreshFailed = fn;
}

/** Attempt to refresh the token. Returns true if a new token was obtained. */
export async function tryRefresh(): Promise<boolean> {
  const ok = await refreshAccessToken();
  if (!ok) _onRefreshFailed?.();
  return ok;
}
