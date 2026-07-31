import { describe, expect, it } from "vitest";
import { domainFromIssuer, readServerAuth, type ProviderFallback } from "./mode";

/** The tenant compiled into the bundle. Every case below is about when this is
 *  used and when the server's answer replaces it. */
const fallback: ProviderFallback = {
  domain: "built-in.example.auth0.com",
  audience: "https://graph-owl.dev/api",
  clientId: "built-in-client",
};

describe("what the server says it will verify", () => {
  it("signs nobody in when the server accepts everyone", () => {
    expect(readServerAuth({ mode: "open" }, fallback)).toEqual({ kind: "open" });
  });

  /** **The reported bug.** A server verifying HS256 cannot accept a token any
   *  identity provider issues, so sending the user to one produces a loop in
   *  which the provider succeeds, the server refuses, and the console reads the
   *  refusal as "signed out". A pasted token is the only credential that can
   *  work, so it is the only one offered. */
  it("asks for a pasted token when the server verifies a shared secret", () => {
    expect(readServerAuth({ mode: "sharedSecret" }, fallback)).toEqual({ kind: "token" });
  });

  it("uses the provider the server names, not the one compiled in", () => {
    const auth = readServerAuth(
      {
        mode: "oidc",
        issuer: "https://tenant.example.auth0.com/",
        audience: "https://api.example/",
        clientId: "server-said-this",
      },
      fallback,
    );

    expect(auth).toEqual({
      kind: "provider",
      domain: "tenant.example.auth0.com",
      audience: "https://api.example/",
      clientId: "server-said-this",
    });
  });

  /** A server that has not been given a console client id is the deployment
   *  every install ran before this endpoint existed. It must keep working. */
  it("falls back to the built-in client id when the server names none", () => {
    const auth = readServerAuth(
      { mode: "oidc", issuer: "https://tenant.example.auth0.com/", audience: "https://api.example/" },
      fallback,
    );

    expect(auth).toEqual({
      kind: "provider",
      domain: "tenant.example.auth0.com",
      audience: "https://api.example/",
      clientId: "built-in-client",
    });
  });

  it("falls back per field rather than wholesale", () => {
    const auth = readServerAuth({ mode: "oidc", clientId: "server-said-this" }, fallback);

    expect(auth).toEqual({
      kind: "provider",
      domain: "built-in.example.auth0.com",
      audience: "https://graph-owl.dev/api",
      clientId: "server-said-this",
    });
  });

  /** **Fails closed, and this is the test that keeps it closed.** An
   *  unrecognised answer is most likely a server newer than this bundle. The
   *  tempting default is "open", because it renders something — and it would
   *  drop the sign-in screen from a server that is actually demanding a token,
   *  turning a version skew into an apparently unauthenticated console. */
  it.each([
    ["an unknown mode", { mode: "mutual-tls" }],
    ["no mode at all", {}],
    ["null", null],
    ["a string", "open"],
    ["a mode of the wrong type", { mode: 7 }],
  ])("falls back to the built-in provider given %s", (_label, raw) => {
    expect(readServerAuth(raw, fallback)).toEqual({
      kind: "provider",
      ...fallback,
    });
  });

  /** An empty string is not a configured value. Passing it through would build
   *  `https:///authorize`, which fails in a way that names nothing. */
  it("treats an empty issuer as absent rather than as a domain", () => {
    const auth = readServerAuth({ mode: "oidc", issuer: "", audience: "", clientId: "" }, fallback);

    expect(auth).toEqual({ kind: "provider", ...fallback });
  });
});

describe("the domain inside an issuer URL", () => {
  /** The server reports an issuer (`https://tenant/`); the authorize URL is
   *  built from a bare host. Getting this wrong yields
   *  `https://https://tenant//authorize`. */
  it("drops the scheme and the trailing slash", () => {
    expect(domainFromIssuer("https://tenant.example.auth0.com/")).toBe("tenant.example.auth0.com");
  });

  it("accepts an issuer that is already a bare host", () => {
    expect(domainFromIssuer("tenant.example.auth0.com")).toBe("tenant.example.auth0.com");
  });

  /** Some providers issue under a path (`https://host/realms/x`). The
   *  authorize URL needs the host only. */
  it("keeps only the host when the issuer carries a path", () => {
    expect(domainFromIssuer("https://host.example/realms/graph-owl")).toBe("host.example");
  });

  it("has nothing to report for an empty issuer", () => {
    expect(domainFromIssuer("")).toBe("");
  });
});
