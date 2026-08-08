"""Test-only scaffolding for agent-triage's own test suite: JWT minting and
admin setup calls the reference application itself must never make (it
answers questions about assets that already exist; it does not create the
scenario it is being asked about). Exempt from Slice A's purity check —
see `check-examples-purity.py`'s own comment on why a test harness is
scaffolding, not the application.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import subprocess
import urllib.error
import urllib.request

JWT_SECRET = "graph-owl-example-fixture-secret-not-for-production"
ADMIN_SUBJECT = "admin"

_ADMIN_BOOTSTRAPPED = False


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def mint_token(subject: str) -> str:
    """A minimal HS256 JWT — stdlib `hmac`/`hashlib` only, matching
    `graph-owl-server`'s own signing (`AUTHZ_FIXTURE_SECRET`-style test
    tokens: `{sub, name, exp}`, HS256). No third-party JWT library needed
    for three fields and one signature."""
    header = _b64url(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    payload = _b64url(
        json.dumps({"sub": subject, "name": subject, "exp": 4_102_444_800}).encode()  # year 2100
    )
    signing_input = f"{header}.{payload}".encode()
    signature = _b64url(hmac.new(JWT_SECRET.encode(), signing_input, hashlib.sha256).digest())
    return f"{header}.{payload}.{signature}"


def base_url() -> str:
    return os.environ.get("GRAPH_OWL_TEST_ENDPOINT", "http://127.0.0.1:8080")


def bootstrap_admin() -> None:
    """Grant `admin` real admin rights, once per test run — by direct SQL,
    the mechanism `plans/12-13-security.md` itself names as the sanctioned
    way to grant the very first role ("Granting the first role required
    direct SQL").

    **Not a workaround for a bug** — checked before writing this, not
    assumed: `GRAPH_OWL_ADMIN_SUBJECTS` bootstrap-admin only applies on the
    OIDC verification path (`graph-owl-server::verify_jwks`); the
    shared-secret HS256 path (`authenticate_bearer_token`'s other branch)
    never calls it. `AuthMode::SharedSecret` is documented as "legacy, and
    a demo affordance" and the security plan pairs `GRAPH_OWL_ADMIN_SUBJECTS`
    with OIDC specifically ("unset GRAPH_OWL_JWT_SECRET") — this is a
    deliberate scope boundary, not an oversight, so this test harness works
    within it rather than "fixing" a boundary nobody asked to have widened.
    """
    global _ADMIN_BOOTSTRAPPED
    if _ADMIN_BOOTSTRAPPED:
        return
    container = os.environ["GRAPH_OWL_EXAMPLES_CONTAINER"]
    subprocess.run(
        [
            "docker",
            "exec",
            container,
            "psql",
            "-U",
            "postgres",
            "-d",
            "graphowl",
            "-c",
            "INSERT INTO users (id, display_name, is_admin) VALUES "
            f"('{ADMIN_SUBJECT}', 'admin', TRUE) "
            "ON CONFLICT (id) DO UPDATE SET is_admin = TRUE",
        ],
        check=True,
        capture_output=True,
    )
    _ADMIN_BOOTSTRAPPED = True


def admin_call(method: str, path: str, body: dict | None = None) -> dict:
    """One admin-authenticated HTTP call — setup only, never how the
    reference application itself talks to graph-owl (it goes through MCP,
    `mcp_client.McpClient`, exclusively)."""
    bootstrap_admin()
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(
        f"{base_url()}{path}",
        data=data,
        method=method,
        headers={
            "content-type": "application/json",
            "authorization": f"Bearer {mint_token(ADMIN_SUBJECT)}",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode(errors="replace")
        raise RuntimeError(f"{method} {path} -> {exc.code}: {detail}") from exc


def grant_role(subject: str, roles: list[str]) -> None:
    admin_call("PUT", f"/users/{subject}", {"displayName": subject})
    admin_call("PUT", f"/users/{subject}/roles", {"roles": roles})


def allow_all_for_role(policy_name: str, role: str) -> None:
    """A baseline "can see everything" policy — found necessary, not
    assumed: this catalog's authorization **denies by default**
    (`plans/12-13-security.md`: "a completely successful sign-in renders
    as an empty estate"). A non-admin principal with no role and no policy
    sees nothing at all, confirmed directly against a real server before
    writing this — `search_assets` returned zero hits for every query
    until a role carrying an allow-all policy existed. A reference
    application's own test scenarios need a principal who *can* see things
    to be meaningful, the same way a real deployment needs a baseline
    viewer policy before anyone but an admin can use the catalog."""
    admin_call(
        "POST",
        "/policies",
        {
            "policy": {
                "name": policy_name,
                "rules": [
                    {
                        "name": f"{policy_name}-allow",
                        "effect": "allow",
                        "operations": ["viewBasic", "viewDetails"],
                        "resources": {"type": "all"},
                    }
                ],
            },
            "roles": [role],
        },
    )


def deny_fqn_prefix_for_role(policy_name: str, role: str, fqn_prefix: str) -> None:
    """A policy admitting everything *except* `fqn_prefix`, attached to
    `role`. Deny-then-allow order matters: `graph_owl_authz`'s own
    predicate composition resolves the *narrowest* matching rule, and an
    `allow(all)` rule ahead of a `deny(prefix)` one must not shadow it —
    ordering the deny rule first is the same convention this project's own
    fixtures use elsewhere for exactly this reason."""
    admin_call(
        "POST",
        "/policies",
        {
            "policy": {
                "name": policy_name,
                "rules": [
                    {
                        "name": f"{policy_name}-deny",
                        "effect": "deny",
                        "operations": ["viewBasic", "viewDetails"],
                        "resources": {"type": "fqnPrefix", "value": fqn_prefix},
                    },
                    {
                        "name": f"{policy_name}-allow-rest",
                        "effect": "allow",
                        "operations": ["viewBasic", "viewDetails"],
                        "resources": {"type": "all"},
                    },
                ],
            },
            "roles": [role],
        },
    )
