import { describe, expect, it } from "vitest";
import {
  type PkceHandoff,
  authorizeUrl,
  base64UrlEncode,
  readCallback,
  rememberHandoff,
  stateMatches,
  takeHandoff,
} from "./pkce";

/** A `Storage` that is not the browser's, so a test never depends on one. */
function storage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => entries.clear(),
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => entries.delete(key),
    setItem: (key: string, value: string) => entries.set(key, value),
  } satisfies Storage;
}

function handoff(overrides?: Partial<PkceHandoff>): PkceHandoff {
  return { state: "state-abc", verifier: "verifier-xyz", ...overrides };
}

describe("surviving the redirect", () => {
  /** The bug this module exists to fix: `login()` navigates away, the JS
   *  context is destroyed, and anything held in a module variable is gone by
   *  the time the callback loads. */
  it("hands the state and verifier back after the context is thrown away", () => {
    const store = storage();
    rememberHandoff(store, handoff());

    // Nothing in memory survives; only what was parked.
    expect(takeHandoff(store)).toEqual({
      state: "state-abc",
      verifier: "verifier-xyz",
    });
  });

  /** Single-use by construction, not by discipline. A verifier left behind is
   *  available to whatever lands on the callback next. */
  it("consumes the material as it reads it, so a second take finds nothing", () => {
    const store = storage();
    rememberHandoff(store, handoff());

    takeHandoff(store);

    expect(takeHandoff(store)).toEqual({});
    expect(store.length).toBe(0);
  });

  it("a callback with nothing parked yields nothing, rather than a partial", () => {
    expect(takeHandoff(storage())).toEqual({});
  });

  /** `sessionStorage` is shared by everything on the origin. An unnamespaced
   *  key — or worse, the empty one — collides with whatever else the host
   *  serves, and the collision shows up as a sign-in that fails intermittently
   *  for one tenant on a shared domain. */
  it("parks under keys namespaced to this application", () => {
    const store = storage();
    rememberHandoff(store, handoff());

    const keys = Array.from({ length: store.length }, (_, i) => store.key(i));

    expect(keys).toHaveLength(2);
    expect(new Set(keys).size).toBe(2);
    for (const key of keys) {
      expect(key).toMatch(/^graphowl\./);
    }
  });
});

describe("the state check, which is the CSRF defence", () => {
  it("matches what was sent", () => {
    expect(stateMatches("abc", "abc")).toBe(true);
  });

  it("rejects a different value", () => {
    expect(stateMatches("abc", "xyz")).toBe(false);
  });

  /** The failure that must not reach the success branch. An attacker who makes
   *  the browser hit our callback with *their* authorization code logs the
   *  victim into the attacker's account, and everything after looks normal.
   *  "Nothing to compare against" is not "it matched". */
  it("rejects when nothing was stored, rather than passing vacuously", () => {
    expect(stateMatches("abc", null)).toBe(false);
    expect(stateMatches("abc", "")).toBe(false);
  });

  it("rejects an empty returned state even against an empty stored one", () => {
    expect(stateMatches("", "")).toBe(false);
  });
});

describe("reading a callback", () => {
  it("finds a code and its state", () => {
    expect(readCallback("?code=abc&state=xyz")).toEqual({
      kind: "code",
      code: "abc",
      state: "xyz",
    });
  });

  /** The provider's own failure has to be distinguishable. "You denied
   *  consent" and "this client is misconfigured" are different problems, and a
   *  callback that returns silently to the sign-in screen tells the user
   *  neither. */
  it("surfaces a provider error with its description", () => {
    expect(
      readCallback("?error=access_denied&error_description=User%20declined"),
    ).toEqual({
      kind: "error",
      error: "access_denied",
      description: "User declined",
    });
  });

  it("an error without a description is still an error", () => {
    expect(readCallback("?error=server_error")).toEqual({
      kind: "error",
      error: "server_error",
      description: null,
    });
  });

  /** An error takes precedence over a code. A provider that sent both is
   *  reporting a failure, and exchanging the code anyway would act on the half
   *  of the message we preferred. */
  it("prefers the error when a response carries both", () => {
    expect(readCallback("?error=access_denied&code=abc&state=xyz").kind).toBe("error");
  });

  it("a code without a state is not a callback, because it cannot be checked", () => {
    expect(readCallback("?code=abc")).toEqual({ kind: "none" });
  });

  /** And the mirror image, which is the dangerous one: a `state` with no code
   *  must not be read as a code callback. Treating it as one sends `null` to
   *  the token endpoint and burns the parked verifier on a request that was
   *  never going to succeed. */
  it("a state without a code is not a callback either", () => {
    expect(readCallback("?state=xyz")).toEqual({ kind: "none" });
  });

  it("an ordinary page load is not a callback", () => {
    expect(readCallback("")).toEqual({ kind: "none" });
    expect(readCallback("?asset=123&theme=dark")).toEqual({ kind: "none" });
  });
});

describe("the authorize URL", () => {
  const params = {
    domain: "tenant.us.auth0.com",
    clientId: "client-1",
    redirectUri: "http://localhost:8080/callback",
    audience: "https://graph-owl.dev/api",
    scope: "openid profile",
    state: "state-abc",
    challenge: "challenge-xyz",
  };

  it("carries every parameter the provider needs", () => {
    const url = new URL(authorizeUrl(params));

    expect(url.origin).toBe("https://tenant.us.auth0.com");
    expect(url.pathname).toBe("/authorize");
    expect(url.searchParams.get("response_type")).toBe("code");
    expect(url.searchParams.get("client_id")).toBe("client-1");
    expect(url.searchParams.get("redirect_uri")).toBe("http://localhost:8080/callback");
    expect(url.searchParams.get("audience")).toBe("https://graph-owl.dev/api");
    expect(url.searchParams.get("state")).toBe("state-abc");
    expect(url.searchParams.get("code_challenge")).toBe("challenge-xyz");
  });

  /** `plain` makes the challenge *be* the verifier, so anyone who can read the
   *  authorization request can complete the exchange — the entire attack PKCE
   *  exists to stop. */
  it("always asks for S256, never plain", () => {
    expect(new URL(authorizeUrl(params)).searchParams.get("code_challenge_method")).toBe(
      "S256",
    );
  });

  it("escapes a redirect URI rather than splicing it in raw", () => {
    const url = authorizeUrl({ ...params, redirectUri: "http://x/cb?a=b&c=d" });

    expect(url).not.toContain("cb?a=b&c=d");
    expect(new URL(url).searchParams.get("redirect_uri")).toBe("http://x/cb?a=b&c=d");
  });
});

describe("base64url", () => {
  /** Asserted as an exact string, not as an absence.
   *
   *  `expect(encoded).not.toContain("+")` is satisfied by *deleting* every
   *  `+` rather than translating it — which produces a shorter string that
   *  decodes to different bytes, and a verifier whose challenge will never
   *  match. The substitution has to be checked as a substitution. */
  it("translates the two unsafe characters rather than deleting them", () => {
    // 0xFB 0xFF 0xBF is exactly "+/+/" in standard base64.
    expect(base64UrlEncode(new Uint8Array([0xfb, 0xff, 0xbf]))).toBe("-_-_");
  });

  it("strips padding without appending anything in its place", () => {
    // 0xFB alone is "+w==" in standard base64.
    expect(base64UrlEncode(new Uint8Array([0xfb]))).toBe("-w");
  });

  it("round-trips through the browser's decoder", () => {
    const bytes = new Uint8Array([0, 1, 127, 128, 254, 255]);
    const encoded = base64UrlEncode(bytes);
    const decoded = atob(encoded.replace(/-/g, "+").replace(/_/g, "/"));

    expect([...decoded].map((c) => c.charCodeAt(0))).toEqual([...bytes]);
  });

  it("encodes nothing as nothing", () => {
    expect(base64UrlEncode(new Uint8Array([]))).toBe("");
  });
});
