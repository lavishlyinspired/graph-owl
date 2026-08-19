"""The invoice's own neighbourhood, in the console's own visual pattern.

**The pattern, from the delivered screenshots**: a circular badge carrying a
short type code, the entity's own label beneath it, a small `namespace:Type`
line beneath that, and an edge drawn as a line labelled with its predicate —
some edges also carry a confidence score and a dashed style.

graph-owl already exposes this at `POST /graph/context`, seeded on any
subject, and the console's own Explore screen already resolves the same
badge/label/sub-label shape via `[console.labels]`. This module is the
translation from graph-owl's raw `{id, iri, sources, label}` /
`{from, to, relationship, derived}` shape into what an SVG renderer needs —
not a second graph, the same one, reshaped.

**No node types are invented.** The reference screenshot's DOC/ORG/SRC/CO/GEO
nodes come from a different pack entirely; this pack has no ERP table or
company registry. The badge is derived from whatever class the subject
actually carries in `packs/gst`.
"""

from __future__ import annotations

import os
import uuid

import asyncpg
import pytest
from fastapi.testclient import TestClient

from app.case_graph import BADGE_FOR_CLASS, badge_for, build_picture

ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


@pytest.fixture
async def client():
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()
    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as test_client:
            yield test_client
    finally:
        del os.environ["DATABASE_URL"]
        admin_conn = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin_conn.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin_conn.close()


class TestBadges:
    def test_a_known_gst_class_gets_its_own_short_code(self):
        assert badge_for("gst:PurchaseInvoice") == "INV"
        assert badge_for("gst:Supplier") == "ORG"
        assert badge_for("gst:Gstr2bStatement") == "DOC"
        assert badge_for("gst:FilingPeriod") == "PER"

    def test_an_unmapped_class_falls_back_to_its_first_three_letters(self):
        """A class Slice whatever adds tomorrow must still render something,
        not blank."""
        assert badge_for("gst:SomethingBrandNew") == "SOM"

    def test_no_class_at_all_falls_back_to_a_generic_badge(self):
        assert badge_for(None) == "?"

    def test_every_mapped_badge_is_three_characters_or_fewer(self):
        """The circle is small. A longer badge would either overflow it or
        need to shrink the type it is supposed to make legible at a glance."""
        for code in BADGE_FOR_CLASS.values():
            assert len(code) <= 3


NODES = [
    {"id": "books-inv-1", "iri": "https://graph-owl.dev/packs/gst#books-inv-1",
     "sources": ["gst:PurchaseInvoice"], "label": "INV-MAR-011"},
    {"id": "supplier-1", "iri": "https://graph-owl.dev/packs/gst#supplier-1",
     "sources": ["gst:Supplier"], "label": "Sharma Infrastructure Pvt Ltd"},
    {"id": "filing-1", "iri": "https://graph-owl.dev/packs/gst#gstr2b-statement-2026-03",
     "sources": ["gst:Gstr2bStatement"], "label": "GSTR-2B — 2026-03"},
]
EDGES = [
    {"from": "books-inv-1", "to": "supplier-1", "relationship": "issuedBy", "derived": False},
    {"from": "books-inv-1", "to": "filing-1", "relationship": "reflectedIn", "derived": True},
]


CLASSES = {
    "books-inv-1": "gst:PurchaseInvoice",
    "supplier-1": "gst:Supplier",
    "filing-1": "gst:Gstr2bStatement",
}


class TestThePicture:
    def test_every_node_carries_a_badge_a_label_and_a_type_line(self):
        """**The class is looked up, not read off `sources`.** graph-owl's
        `sources` field on a context node is the named *import graphs* the
        subject appears in (`reco-abc-books`) — nothing about its RDF class.
        Trusting it produced the same generic badge for every node on real
        data, which defeats the entire point of the pattern."""
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        supplier = next(n for n in picture["nodes"] if n["id"] == "supplier-1")
        assert supplier["badge"] == "ORG"
        assert supplier["label"] == "Sharma Infrastructure Pvt Ltd"
        assert supplier["type_line"] == "gst:Supplier"

    def test_a_node_with_no_resolved_class_still_renders_with_a_generic_badge(self):
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes={})

        supplier = next(n for n in picture["nodes"] if n["id"] == "supplier-1")
        assert supplier["badge"] == "?"
        assert supplier["type_line"] is None

    def test_the_seed_is_marked_so_the_renderer_can_highlight_it(self):
        """The screenshot's cyan ring and cyan edges — the node the drawer was
        actually opened for, distinguished from everything reached from it."""
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        seed_node = next(n for n in picture["nodes"] if n["id"] == "books-inv-1")
        assert seed_node["is_seed"] is True
        other = next(n for n in picture["nodes"] if n["id"] != "books-inv-1")
        assert other["is_seed"] is False

    def test_an_edge_touching_the_seed_is_marked_highlighted(self):
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        assert all(e["highlighted"] for e in picture["edges"])

    def test_an_edge_not_touching_the_seed_is_not_highlighted(self):
        far_edge = [{"from": "supplier-1", "to": "filing-1", "relationship": "x", "derived": False}]
        picture = build_picture(
            seed="books-inv-1", nodes=NODES, edges=EDGES + far_edge, classes=CLASSES
        )

        far = next(e for e in picture["edges"] if e["from"] == "supplier-1")
        assert far["highlighted"] is False

    def test_a_derived_edge_is_styled_dashed(self):
        """`reflectedIn` here came from a rule's own derivation rather than a
        direct import assertion — `derived` on graph-owl's own edge shape.
        The dashed line in the reference screenshot (hasPAN, locatedIn) is
        exactly this distinction: inferred, not asserted."""
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        reflected = next(e for e in picture["edges"] if e["label"] == "reflectedIn")
        assert reflected["style"] == "dashed"

    def test_a_direct_edge_is_styled_solid(self):
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        issued = next(e for e in picture["edges"] if e["label"] == "issuedBy")
        assert issued["style"] == "solid"

    def test_the_predicates_own_local_name_is_the_edge_label(self):
        """`https://graph-owl.dev/...#issuedBy` on screen as `issuedBy`, the
        way the reference screenshot labels `declaredIn`/`masteredAs` — not
        the full IRI, which nobody reads at a glance."""
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        assert all("http" not in e["label"] for e in picture["edges"])

    def test_a_node_with_no_recognisable_source_still_renders(self):
        """graph-owl's own `sources` can be empty for a subject nothing has
        typed yet. A blank picture is worse than a generic badge."""
        bare = [{"id": "x", "iri": "urn:x", "sources": [], "label": "x"}]
        picture = build_picture(seed="x", nodes=bare, edges=[], classes={})

        assert picture["nodes"][0]["badge"] == "?"

    def test_nodes_and_edges_absent_from_the_seed_component_are_still_included(self):
        """graph-owl already bounds the walk; this module reshapes what comes
        back rather than filtering it a second time."""
        picture = build_picture(seed="books-inv-1", nodes=NODES, edges=EDGES, classes=CLASSES)

        assert len(picture["nodes"]) == 3
        assert len(picture["edges"]) == 2


class TestTheEndpoint:
    """Against a real database and the real graph-owl the fixtures already run
    — the layer the pure tests above cannot reach."""

    def test_a_case_with_no_recorded_subject_returns_an_empty_picture(self, client):
        client_id, period_id = _period(client)
        response = client.post(
            f"/api/clients/{client_id}/periods/{period_id}/cases",
            json={
                "invoice_no": "INV-1", "reason_code": "gst:AmountMismatch",
                "supplier_gstin": "27AABCS1429B1Z8", "supplier_name": "Sharma",
            },
        )
        case_id = response.json()["id"]

        result = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/cases/{case_id}/graph"
        ).json()

        assert result["nodes"] == []
        assert result["seed"] is None

    def test_an_unknown_case_is_404(self, client):
        client_id, period_id = _period(client)

        response = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/cases/does-not-exist/graph"
        )

        assert response.status_code == 404


def _period(client) -> tuple[str, str]:
    created = client.post(
        "/api/clients",
        json={"name": "Graph Co", "gstin": "27AABCU9603R1ZM", "state": "Maharashtra"},
    ).json()
    period = client.post(
        f"/api/clients/{created['id']}/periods", json={"month": "March", "year": 2026}
    ).json()
    return created["id"], period["id"]
