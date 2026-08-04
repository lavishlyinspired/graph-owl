#!/usr/bin/env python3
"""Epic 7d Slice F: prove a real, unmodified property-graph driver works.

A hand-rolled test client — everything in `crates/graph-owl-server/tests/bolt.rs`
— can be wrong in exactly the same way the server is and prove nothing about
real-world compatibility. This script is the acceptance for the epic: the
official `neo4j` Python driver (Apache-2.0, PyPI), talking to a live
`graph-owl-server` binary over a real socket, with no graph-owl-specific code
in the client at all.

Seeds a couple of `Service`-kind assets over the REST API — the cheapest
fixture that actually projects into the graph (`Catalog::upsert_asset`
projects on every write; the older, table-specific `POST /tables` walking
skeleton does not project at all, and relationships are reachable only
through that unprojected path today — a real gap, not a scope choice, and
out of what this slice covers) — then drives the driver through:

  1. connect + authenticate (open mode; auth itself is covered exhaustively
     in `bolt.rs`'s HELLO tests, including the identity-equivalence one)
  2. a query returning a whole node, read back through the driver's own
     typed `Node` object — labels, properties, element_id
  3. an explicit transaction (`session.begin_transaction()` / `commit()`)
  4. a write clause, refused

Invoked by `scripts/verify-bolt.sh`, which starts the server this connects
to. Exits non-zero on any failure.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request

import neo4j

REST_BASE = os.environ["GRAPH_OWL_BASE_URL"]
BOLT_URI = os.environ["GRAPH_OWL_BOLT_URI"]


def rest_post(path: str, body: dict) -> dict:
    request = urllib.request.Request(
        f"{REST_BASE}{path}",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request) as response:
        return json.loads(response.read())


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)
    print(f"  ok: {message}")


def main() -> int:
    print("==> seeding two service assets over the REST API")
    rest_post("/assets", {"kind": "service", "name": "bolt-check-alpha"})
    rest_post("/assets", {"kind": "service", "name": "bolt-check-beta"})

    driver = neo4j.GraphDatabase.driver(BOLT_URI, auth=None)
    try:
        print("==> connecting and authenticating")
        driver.verify_connectivity()
        check(True, "the official driver completed its own handshake and HELLO")

        print("==> a whole node comes back correctly typed")
        with driver.session() as session:
            # A literal in the query text, not a driver-side `$parameter` —
            # this Cypher subset's expression lowering has no case for
            # `Parameter` yet (`decypher` parses the syntax; nothing lowers
            # it), a real gap found running this script, not a scope choice.
            # Every value here is one this process generated, not user
            # input, so a literal is safe in this context.
            records = list(session.run("MATCH (n:service) WHERE n.name = 'bolt-check-alpha' RETURN n"))
            check(len(records) == 1, f"exactly one match for the seeded asset, got {len(records)}")
            node = records[0]["n"]
            check(isinstance(node, neo4j.graph.Node), "the driver's own Node type, not a string")
            check("service" in node.labels, f"label carries the asset kind: {list(node.labels)}")
            check(node["name"] == "bolt-check-alpha", f"properties are readable by name: {dict(node)}")
            check(isinstance(node.element_id, str) and node.element_id, "element_id is a non-empty string")

        print("==> an explicit transaction commits")
        with driver.session() as session:
            tx = session.begin_transaction()
            rows = list(
                tx.run("MATCH (n:service) WHERE n.name = 'bolt-check-beta' RETURN n.name AS name")
            )
            check(len(rows) == 1 and rows[0]["name"] == "bolt-check-beta", "the explicit transaction reads correctly")
            tx.commit()
            check(True, "COMMIT completes without error")

        print("==> a write clause is refused, not silently accepted")
        with driver.session() as session:
            try:
                session.run("CREATE (n:service {name: 'should-not-exist'})").consume()
                raise AssertionError("a write clause must be refused")
            except neo4j.exceptions.Neo4jError as error:
                check(True, f"refused as expected: {error}")

    finally:
        driver.close()

    print(
        "==> ok: a real, unmodified driver connects, authenticates, queries, "
        "reads a typed node, transacts, and is refused a write"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
