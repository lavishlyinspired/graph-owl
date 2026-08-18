import { createContext, useContext, useEffect, useState, useSyncExternalStore } from "react";
import { fetchAuthConfig, setTokenSource } from "../api";
import { readServerAuth, type ProviderFallback, type ServerAuth } from "./mode";
import {
  authorizeUrl,
  base64UrlEncode,
  readCallback,
  rememberHandoff,
  stateMatches,
  takeHandoff,
} from "./pkce";
import { rememberSession, storedSession } from "./session";

/** Build-time configuration, not literals.
 *
 *  A public SPA client id is not a secret, but a tenant baked into source is
 *  still a code change for every deployment that is not this one — and the
 *  fallbacks below are a development tenant, which is exactly the value that
 *  must not silently ship to production.
 *
 *  **These are now a fallback rather than the answer.** The server reports what
 *  it will actually verify (`GET /auth/config`); these are what the console uses
 *  when it cannot ask — an older server, or an unreadable reply. See `mode.ts`
 *  for why that fallback is a provider and never "open". */
const BUILT_IN: ProviderFallback = {
  domain: import.meta.env.VITE_OIDC_DOMAIN ?? "dev-uzuxwkbcozynti2m.us.auth0.com",
  clientId: import.meta.env.VITE_OIDC_CLIENT_ID ?? "1YagciKTz39X5IlvZnYjFylGgzJRVkI6",
  audience: import.meta.env.VITE_OIDC_AUDIENCE ?? "https://graph-owl.dev/api",
};
const SCOPE = "openid profile email offline_access";
const REDIRECT_URI = `${window.location.origin}/callback`;

/** The provider actually in use. Module-level because the token exchange and
 *  the refresh both run outside React, and a redirect destroys the component
 *  that discovered it. Starts as the built-in tenant so a callback that lands
 *  before discovery finishes still has somewhere to exchange its code. */
let _provider: ProviderFallback = BUILT_IN;

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
  /** How this server wants a credential obtained, which decides *which* screen
   *  `unauthenticated` shows — an identity provider button is useless against a
   *  server that verifies a shared secret. `open` never reaches
   *  `unauthenticated` at all. */
  strategy: "provider" | "token" | "open";
}

interface AuthContextValue {
  state: AuthState;
  login: () => void;
  logout: () => void;
  /** Accept a token supplied by hand. Shared-secret servers only: no
   *  interactive provider can mint one, and `demo.sh --secure` prints two. */
  signInWithToken: (token: string) => void;
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
    domain: _provider.domain,
    clientId: _provider.clientId,
    redirectUri: REDIRECT_URI,
    audience: _provider.audience,
    scope: SCOPE,
    state,
    challenge,
  });
}

async function exchangeCode(code: string, codeVerifier: string): Promise<void> {
  const response = await fetch(`https://${_provider.domain}/oauth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      client_id: _provider.clientId,
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
  rememberSession(window.sessionStorage, _refreshToken);
  notify();
}

async function refreshAccessToken(): Promise<boolean> {
  if (_refreshInFlight) return _refreshInFlight;

  _refreshInFlight = (async () => {
    const rt = _refreshToken;
    if (!rt) return false;

    try {
      const response = await fetch(`https://${_provider.domain}/oauth/token`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          grant_type: "refresh_token",
          client_id: _provider.clientId,
          refresh_token: rt,
        }),
      });

      if (!response.ok) {
        _accessToken = null;
        _refreshToken = null;
        rememberSession(window.sessionStorage, null);
        notify();
        return false;
      }

      const data = await response.json();
      _accessToken = data.access_token ?? null;
      // Auth0 rotates refresh tokens: the one just used is now spent, so a
      // response carrying a new one must replace the stored copy or the next
      // reload restores a token the provider has already revoked.
      if (data.refresh_token) {
        _refreshToken = data.refresh_token;
        rememberSession(window.sessionStorage, data.refresh_token);
      }
      notify();
      return true;
    } catch {
      _accessToken = null;
      _refreshToken = null;
      rememberSession(window.sessionStorage, null);
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

/** Is there a session to restore?
 *
 *  Read once at module scope, before React renders, for the same reason
 *  `arrivedOnCallback` is: the first paint has to be "restoring" rather than a
 *  flash of the sign-in screen the user is about to be silently signed past. */
const hasStoredSession = arrivedOnCallback.kind === "none" && storedSession(window.sessionStorage) !== null;

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const token = useSyncExternalStore(subscribeToTokenChanges, getTokenSnapshot);
  const [exchange, setExchange] = useState<{
    status: "idle" | "exchanging";
    error: string | null;
  }>(
    arrivedOnCallback.kind === "code" || hasStoredSession
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
  // Restore a session the previous page load left behind, *before* concluding
  // the user is signed out. Without this a reload sends somebody who is still
  // signed in with the provider back through a login they already completed.
  useEffect(() => {
    if (!hasStoredSession) return;
    const stored = storedSession(window.sessionStorage);
    if (!stored) return;

    _refreshToken = stored;
    let live = true;
    refreshAccessToken().then((restored) => {
      if (!live) return;
      // A refresh token the provider has revoked is not an error worth showing:
      // the session simply ended, and the sign-in screen is the right answer.
      setExchange({ status: "idle", error: restored ? null : null });
    });
    return () => {
      live = false;
    };
  }, []);

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

  // What the *server* will verify, which is the question the console never
  // previously asked. `null` means the answer has not arrived yet.
  const [server, setServer] = useState<ServerAuth | null>(null);

  useEffect(() => {
    let live = true;
    fetchAuthConfig()
      .then((raw) => (live ? readServerAuth(raw, BUILT_IN) : null))
      .catch(() =>
        // A server too old to answer, or an unreadable reply. Both degrade to
        // the behaviour that shipped before this endpoint existed rather than
        // to an unauthenticated console — see `mode.ts`.
        live ? ({ kind: "provider", ...BUILT_IN } as ServerAuth) : null,
      )
      .then((resolved) => {
        if (!live || resolved === null) return;
        if (resolved.kind === "provider") {
          _provider = {
            domain: resolved.domain,
            audience: resolved.audience,
            clientId: resolved.clientId,
          };
        }
        setServer(resolved);
      });
    return () => {
      live = false;
    };
  }, []);

  const authState: AuthState = ((): AuthState => {
    // Discovery is a loading state, not a signed-out one. Rendering the sign-in
    // screen first would flash the very screen this whole change exists to stop
    // people bouncing off.
    if (server === null) return { status: "loading", error: null, strategy: "provider" };

    // An open server authenticates nobody, so there is nothing to wait for and
    // no credential to collect.
    if (server.kind === "open") return { status: "authenticated", error: null, strategy: "open" };

    const strategy = server.kind === "token" ? "token" : "provider";
    if (token) return { status: "authenticated", error: null, strategy };
    if (exchange.status === "exchanging") return { status: "loading", error: null, strategy };
    return { status: "unauthenticated", error: exchange.error, strategy };
  })();

  const value: AuthContextValue = {
    state: authState,
    login: async () => {
      // Only a provider flow is a redirect. Against a shared-secret server the
      // sign-in screen collects a token directly, and against an open one there
      // is nothing to do — starting a redirect in either case is what produced
      // the loop.
      if (authState.strategy !== "provider") return;

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
    signInWithToken: (pasted: string) => {
      const trimmed = pasted.trim();
      if (trimmed.length === 0) return;
      _accessToken = trimmed;
      // No refresh token, and none is invented. A hand-supplied token has no
      // grant behind it, so when it expires the honest answer is to ask for
      // another one rather than to attempt a refresh that cannot succeed.
      _refreshToken = null;
      rememberSession(window.sessionStorage, null);
      notify();
    },
    logout: () => {
      _accessToken = null;
      _refreshToken = null;
      rememberSession(window.sessionStorage, null);
      notify();

      // Only a provider has a session of its own to end. Redirecting a
      // shared-secret console to an identity provider's logout endpoint would
      // send it to a tenant it never signed in to, which lands on that
      // provider's error page rather than back here.
      if (authState.strategy !== "provider") return;

      const params = new URLSearchParams({
        client_id: _provider.clientId,
        returnTo: window.location.origin,
      });
      window.location.href = `https://${_provider.domain}/v2/logout?${params}`;
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
