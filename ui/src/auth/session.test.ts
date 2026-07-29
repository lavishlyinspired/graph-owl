import { beforeEach, describe, expect, it } from "vitest";
import { rememberSession, storedSession } from "./session";

/** An in-memory `Storage`, so these tests say nothing about a browser. */
function storage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => entries.clear(),
    getItem: (key) => entries.get(key) ?? null,
    key: (index) => [...entries.keys()][index] ?? null,
    removeItem: (key) => {
      entries.delete(key);
    },
    setItem: (key, value) => {
      entries.set(key, value);
    },
  };
}

/** A `Storage` that refuses everything — private mode, a quota, a policy. */
function refusing(): Storage {
  const deny = () => {
    throw new DOMException("denied", "SecurityError");
  };
  return {
    length: 0,
    clear: deny,
    getItem: deny,
    key: deny,
    removeItem: deny,
    setItem: deny,
  };
}

describe("a session survives a reload", () => {
  let store: Storage;
  beforeEach(() => {
    store = storage();
  });

  // The whole point. Without it the next page load has no session, falls back
  // to the sign-in screen, and bounces the user through a login they already
  // completed — which reads as "it did not work".
  it("hands back the token the last load parked", () => {
    rememberSession(store, "rt-abc");

    expect(storedSession(store)).toBe("rt-abc");
  });

  it("reports no session when nothing was parked", () => {
    expect(storedSession(store)).toBeNull();
  });

  // Signing out has to end the session, not merely stop using it. A token left
  // behind is one the next load happily restores.
  it("clears the token when the session ends", () => {
    rememberSession(store, "rt-abc");

    rememberSession(store, null);

    expect(storedSession(store)).toBeNull();
  });

  // Auth0 rotates refresh tokens: the one just used is spent. Keeping the old
  // one restores a credential the provider has already revoked, and the failure
  // surfaces one reload later as an unexplained sign-out.
  it("replaces a rotated token rather than keeping the spent one", () => {
    rememberSession(store, "rt-first");

    rememberSession(store, "rt-second");

    expect(storedSession(store)).toBe("rt-second");
  });

  // `null` must clear, not store the string "null" — `getItem` returns `null`
  // for both, and a stored "null" would be handed to the provider as a
  // credential.
  it("never stores the word null as a token", () => {
    rememberSession(store, null);

    expect(store.getItem("graphowl.refresh.v1")).toBeNull();
  });

  // An empty string reaches here from a provider that returned one. Sending it
  // back produces a confusing 401 instead of the honest "no session".
  it("treats an empty token as no session", () => {
    store.setItem("graphowl.refresh.v1", "");

    expect(storedSession(store)).toBeNull();
  });

  // Reading is not consuming, unlike the PKCE verifier: a refresh token is used
  // repeatedly, and clearing it on read would end the session on first restore.
  it("does not consume the token on read", () => {
    rememberSession(store, "rt-abc");

    storedSession(store);

    expect(storedSession(store)).toBe("rt-abc");
  });
});

describe("when storage is unavailable", () => {
  // Private mode, a quota, an enterprise policy. A session that does not
  // survive a reload is worse than one that does, and far better than a page
  // that refuses to render.
  it("does not throw when parking a token", () => {
    expect(() => rememberSession(refusing(), "rt-abc")).not.toThrow();
  });

  it("does not throw when clearing", () => {
    expect(() => rememberSession(refusing(), null)).not.toThrow();
  });

  it("reports no session rather than failing", () => {
    expect(storedSession(refusing())).toBeNull();
  });
});
