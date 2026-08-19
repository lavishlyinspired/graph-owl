"""An invoice's own neighbourhood, in the console's own visual pattern.

**The pattern**: a circular badge carrying a short type code, the entity's own
label beneath it, a small `namespace:Type` line beneath that, and an edge drawn
as a line labelled with its predicate — a direct assertion solid, a derived one
dashed.

graph-owl already exposes this at `POST /graph/context`, seeded on any
subject. This module is the reshaping from graph-owl's raw
`{id, iri, sources, label}` / `{from, to, relationship, derived}` into what an
SVG renderer needs — the same graph, not a second one.

**No node types are invented.** A reference mockup for a different pack showed
ERP-table and company-registry nodes; this pack has neither. The badge is
derived from whatever class the subject actually carries.
"""

from __future__ import annotations

from typing import Any

#: Short codes for the classes this pack actually mints. Three characters or
#: fewer — the circle is small, and a longer code would either overflow it or
#: need to shrink the type it exists to make legible at a glance.
BADGE_FOR_CLASS: dict[str, str] = {
    "gst:PurchaseInvoice": "INV",
    "gst:Gstr2bInvoice": "INV",
    "gst:Gstr1Invoice": "INV",
    "gst:Gstr2aInvoice": "INV",
    "gst:Supplier": "ORG",
    "gst:Gstr1Filing": "DOC",
    "gst:Gstr2bStatement": "DOC",
    "gst:Gstr2aSnapshot": "DOC",
    "gst:Gstr3bReturn": "DOC",
    "gst:FilingPeriod": "PER",
    "gst:PaymentEvent": "EVT",
    "gst:GoodsReceipt": "EVT",
    "gst:PurchaseEvent": "EVT",
}


def badge_for(class_name: str | None) -> str:
    """A short code for one class.

    A class this table does not name gets its own first three letters after
    the namespace — a class Slice whatever adds tomorrow must still render
    something rather than a blank circle. No class at all is `"?"`.
    """
    if not class_name:
        return "?"
    if class_name in BADGE_FOR_CLASS:
        return BADGE_FOR_CLASS[class_name]
    local = class_name.split(":")[-1]
    return local[:3].upper()


def _local_name(iri_or_name: str) -> str:
    """The predicate's own local name — `issuedBy`, not the full IRI. Nobody
    reads a full URL at a glance, and the reference pattern labels edges with
    the bare predicate."""
    for sep in ("#", "/"):
        if sep in iri_or_name:
            return iri_or_name.rsplit(sep, 1)[-1]
    return iri_or_name


def build_picture(
    *,
    seed: str,
    nodes: list[dict[str, Any]],
    edges: list[dict[str, Any]],
    classes: dict[str, str],
) -> dict[str, Any]:
    """graph-owl's raw context, reshaped for the SVG renderer.

    Every node and edge graph-owl returned is included — this reshapes what
    came back rather than filtering it a second time; graph-owl already
    bounded the walk.

    `classes` maps node id -> its `rdf:type`, resolved separately (see
    `graphowl_client.node_classes`). **Not read off the node's own `sources`
    field** — that is the named *import graphs* the subject appears in
    (`reco-abc-books`), nothing about its RDF class. Trusting it produced the
    same generic badge for every node on real data, which defeats the entire
    point of the pattern.
    """
    picture_nodes = []
    for node in nodes:
        class_name = classes.get(node["id"])
        picture_nodes.append(
            {
                "id": node["id"],
                "label": node.get("label") or node["id"],
                "badge": badge_for(class_name),
                "type_line": class_name,
                "is_seed": node["id"] == seed,
            }
        )

    picture_edges = []
    for edge in edges:
        picture_edges.append(
            {
                "from": edge["from"],
                "to": edge["to"],
                "label": _local_name(edge["relationship"]),
                # A derived edge came from a rule's own conclusion rather than
                # a direct import assertion — dashed, the same distinction the
                # reference pattern draws for an inferred relationship.
                "style": "dashed" if edge.get("derived") else "solid",
                "highlighted": edge["from"] == seed or edge["to"] == seed,
            }
        )

    return {"seed": seed, "nodes": picture_nodes, "edges": picture_edges}


__all__ = ["BADGE_FOR_CLASS", "badge_for", "build_picture"]
