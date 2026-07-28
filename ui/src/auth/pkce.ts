/** The parts of the PKCE flow that are decisions rather than side effects.
 *
 *  Split out because the flow spans a **full-page redirect**: the browser
 *  leaves for the identity provider and comes back to a freshly-loaded
 *  document, so anything held in a module variable is gone. That is not a
 *  detail — it is the property the flow lives or dies on, and it is only
 *  testable if the decisions are separated from `window.location`. */

/** Where the single-use PKCE material waits out the redirect.
 *
 *  **`sessionStorage`, and this is not a contradiction of "tokens in memory
 *  only".** That rule exists because an access or refresh token is a bearer
 *  credential with a long life: anything that can read it can act as the user
 *  until it expires. The code verifier is neither. It is single-use, lives for
 *  the seconds between two redirects, and is worthless without the matching
 *  authorization code — which the provider will only issue once, to this
 *  origin's registered callback.
 *
 *  Keeping it in memory is not a stronger position, it is a broken one: the
 *  navigation to the provider destroys the JS context, so the verifier the
 *  callback needs is always `null` and login can never complete. */
const STATE_KEY = "graphowl.pkce.state";
const VERIFIER_KEY = "graphowl.pkce.verifier";

export interface PkceHandoff {
  readonly state: string;
  readonly verifier: string;
}

/** What a callback URL is asking us to do. */
export type Callback =
  | { readonly kind: "code"; readonly code: string; readonly state: string }
  /** The provider reported a failure — `error` and `error_description` per
   *  RFC 6749 §4.1.2.1. Surfaced rather than swallowed: "you denied consent"
   *  and "this client is misconfigured" are different problems, and a callback
   *  that silently returns to the sign-in screen tells the user neither. */
  | { readonly kind: "error"; readonly error: string; readonly description: string | null }
  | { readonly kind: "none" };

/** Read a callback out of a query string. */
export function readCallback(search: string): Callback {
  const params = new URLSearchParams(search);
  const error = params.get("error");
  if (error !== null) {
    return { kind: "error", error, description: params.get("error_description") };
  }
  const code = params.get("code");
  const state = params.get("state");
  if (code !== null && state !== null) {
    return { kind: "code", code, state };
  }
  return { kind: "none" };
}

/** Whether a returned `state` matches what we sent.
 *
 *  This is the CSRF defence, and it is the one check whose *failure* mode is
 *  silent: an attacker who can make the browser hit our callback with their own
 *  authorization code logs the victim into the attacker's account, and
 *  everything after that looks like a normal session.
 *
 *  A missing stored state is a mismatch, never a pass. "We have nothing to
 *  compare against" and "it matched" must not reach the same branch. */
export function stateMatches(returned: string, stored: string | null): boolean {
  return stored !== null && stored.length > 0 && returned === stored;
}

/** Park the state and verifier across the redirect. */
export function rememberHandoff(storage: Storage, handoff: PkceHandoff): void {
  storage.setItem(STATE_KEY, handoff.state);
  storage.setItem(VERIFIER_KEY, handoff.verifier);
}

/** Take the parked material, **removing it in the same step**.
 *
 *  Single-use by construction. A verifier left behind after one exchange is a
 *  verifier available to the next thing that lands on the callback, and the
 *  cheapest way to guarantee one use is to make reading it consume it — rather
 *  than to trust every call site to clean up on both the success and the four
 *  failure paths. */
export function takeHandoff(storage: Storage): Partial<PkceHandoff> {
  const state = storage.getItem(STATE_KEY);
  const verifier = storage.getItem(VERIFIER_KEY);
  storage.removeItem(STATE_KEY);
  storage.removeItem(VERIFIER_KEY);
  return {
    ...(state === null ? {} : { state }),
    ...(verifier === null ? {} : { verifier }),
  };
}

export interface AuthorizeParams {
  readonly domain: string;
  readonly clientId: string;
  readonly redirectUri: string;
  readonly audience: string;
  readonly scope: string;
  readonly state: string;
  readonly challenge: string;
}

/** The URL that starts the flow. */
export function authorizeUrl(params: AuthorizeParams): string {
  const query = new URLSearchParams({
    response_type: "code",
    client_id: params.clientId,
    redirect_uri: params.redirectUri,
    scope: params.scope,
    audience: params.audience,
    state: params.state,
    code_challenge: params.challenge,
    // S256, never `plain`. A `plain` challenge is the verifier itself, so an
    // attacker who can read the authorization request can complete the
    // exchange — which is the entire attack PKCE exists to stop.
    code_challenge_method: "S256",
  });
  return `https://${params.domain}/authorize?${query}`;
}

/** base64url, per RFC 7636 — no padding, URL-safe alphabet.
 *
 *  **One equivalent mutant survives here and is left alone**: dropping the `$`
 *  anchor from `/=+$/`. Base64 padding can only ever occur at the end of the
 *  output, so an unanchored match finds the same run. Adding a test for it
 *  would assert something `btoa` makes impossible, which is a test of the
 *  platform rather than of this function. */
export function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
