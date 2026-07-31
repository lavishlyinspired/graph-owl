/** What the *server* says it will accept, and what the console should therefore
 *  do about signing in.
 *
 *  **The bug this exists to fix.** The console ran an OIDC authorization-code
 *  flow unconditionally, against a tenant compiled into the bundle, regardless
 *  of what the server was configured to verify. Pointed at a server in
 *  shared-secret mode — which is what `scripts/demo.sh --secure` starts — every
 *  step succeeded and the whole thing failed: the provider authenticated the
 *  person, issued a valid RS256 token, and the server refused it because it
 *  only ever verifies HS256. The console read that `401` as "signed out" and
 *  returned the user to the sign-in screen they had just completed, forever,
 *  with no error to show them.
 *
 *  The fix is that the verifier decides. The server reports the mode; this
 *  module turns that into the one sign-in strategy that can actually succeed.
 *
 *  Kept separate from `index.tsx` for the reason `pkce.ts` is: these are
 *  decisions, and decisions are only testable when they are not tangled with a
 *  redirect and a React tree. */

/** The tenant compiled into the bundle, used when the server names none. */
export interface ProviderFallback {
  readonly domain: string;
  readonly audience: string;
  readonly clientId: string;
}

/** How this console can obtain a credential the server will accept. */
export type ServerAuth =
  /** The server authenticates nobody. Signing in would be a ceremony that
   *  changes nothing and can only fail. */
  | { readonly kind: "open" }
  /** The server verifies a shared secret held only by the server. No
   *  interactive provider can mint that, so the token has to be supplied by
   *  hand — `demo.sh --secure` prints two. */
  | { readonly kind: "token" }
  /** The server verifies tokens from this provider. */
  | {
      readonly kind: "provider";
      readonly domain: string;
      readonly audience: string;
      readonly clientId: string;
    };

/** The host inside an issuer URL.
 *
 *  The server reports an issuer (`https://tenant/`) because that is what OIDC
 *  defines; `authorizeUrl` builds from a bare host. Without this the console
 *  requests `https://https://tenant//authorize`. */
export function domainFromIssuer(issuer: string): string {
  if (issuer.length === 0) return "";
  const withoutScheme = issuer.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "");
  // A provider may issue under a path (`https://host/realms/x`); the authorize
  // endpoint is on the host, not under the realm path.
  const [host] = withoutScheme.split("/");
  return host ?? "";
}

/** A field the server actually supplied, as opposed to one it sent empty.
 *
 *  An empty string is not a configured value — passed through it would build
 *  `https:///authorize`, which fails without naming anything. */
function supplied(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** Interpret `GET /auth/config`.
 *
 *  **Unrecognised answers fall back to the provider, never to `open`.** The
 *  tempting default is `open`, because it renders the catalog immediately — and
 *  it is the wrong one: an answer this bundle does not understand most likely
 *  comes from a server newer than it, and assuming that server wants no
 *  credential removes the sign-in screen from one that is in fact demanding a
 *  token. Falling back to the provider is exactly the behaviour that shipped
 *  before this endpoint existed, so an old server and an unreadable answer both
 *  degrade to the status quo rather than to an unauthenticated console. */
export function readServerAuth(raw: unknown, fallback: ProviderFallback): ServerAuth {
  const mode =
    typeof raw === "object" && raw !== null
      ? supplied((raw as { mode?: unknown }).mode)
      : null;

  if (mode === "open") return { kind: "open" };
  if (mode === "sharedSecret") return { kind: "token" };

  // `oidc` and anything unrecognised land here together, deliberately: both
  // mean "a provider is involved", and only the details differ.
  const details = (typeof raw === "object" && raw !== null ? raw : {}) as {
    issuer?: unknown;
    audience?: unknown;
    clientId?: unknown;
  };
  const issuer = mode === "oidc" ? supplied(details.issuer) : null;

  // Per field rather than wholesale: a server that names an issuer but no
  // client id has told us something true, and discarding it because the rest is
  // absent would send the console to the wrong tenant.
  return {
    kind: "provider",
    domain: issuer === null ? fallback.domain : domainFromIssuer(issuer),
    audience: (mode === "oidc" ? supplied(details.audience) : null) ?? fallback.audience,
    clientId: (mode === "oidc" ? supplied(details.clientId) : null) ?? fallback.clientId,
  };
}
