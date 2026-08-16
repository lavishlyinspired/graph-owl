"""RED tests for reco-now's column auto-mapping.

Found while wiring the native graph-owl reconciliation engine
(plans/119-architecture-audit.md §5b step 5, parity testing): reco-now's
own `_auto_map` never detected `igst`/`cgst`/`sgst` against a real GSTR-2B
export's own column names — "Integrated Tax"/"Central Tax"/"State/UT Tax"
— because `_FIELD_KEYWORDS` only matched the literal substrings
"igst"/"cgst"/"sgst". This silently zeroed every tax component on the
portal side of every reconciliation run against realistic data (reco-now's
own SAMPLE/gstr2b_mar2026.csv and gstr2b_aug2026.csv both use these exact
header names), producing 0 matches out of a fixture with 7 genuinely
matching invoices — not a graph-owl integration bug, a pre-existing gap
this session's use of realistic sample data happened to surface.
"""

from __future__ import annotations

from app import graphowl_client, main
from app.main import _auto_map, _ingest_to_graphowl, _install_graphowl_pack, _select_results
from app.reconciliation import STATUS_MATCHED


def test_maps_gstr2b_style_tax_component_headers():
    headers = [
        "Invoice No", "Invoice Date", "GSTIN of Supplier", "Supplier Name",
        "Taxable Value", "Integrated Tax", "Central Tax", "State/UT Tax",
        "Cess", "HSN/SAC", "Place of Supply", "IMS Status", "Note Type",
    ]
    mapping = _auto_map(headers)
    assert mapping["igst"] == headers.index("Integrated Tax")
    assert mapping["cgst"] == headers.index("Central Tax")
    assert mapping["sgst"] == headers.index("State/UT Tax")


def test_still_maps_the_books_style_short_headers():
    # The existing convention — must not regress.
    headers = ["Invoice No", "IGST", "CGST", "SGST", "Cess"]
    mapping = _auto_map(headers)
    assert mapping["igst"] == 1
    assert mapping["cgst"] == 2
    assert mapping["sgst"] == 3


class TestSelectResultsCutsOverToNativeFindings:
    """plans/119-architecture-audit.md §9 — reconciliation.py's own
    tolerance/matching math is no longer the primary source `/api/reconcile`
    returns. `_select_results` is the one decision point that chooses
    between the two, kept small and pure so this decision has its own test
    rather than being buried inside the endpoint."""

    def _book(self, **overrides):
        base = {
            "invoice_no": "INV-1", "supplier_gstin": "27AAAFN2938K1Z2",
            "supplier_name": "Nimbus", "taxable": 100000, "igst": 18000,
            "cgst": 0, "sgst": 0, "cess": 0,
        }
        base.update(overrides)
        return base

    def test_a_healthy_graphowl_reconcile_uses_native_findings(self):
        # No findings at all for this invoice — native says Matched, which
        # reconciliation.py's own tolerance math (never consulted here)
        # would not necessarily agree with; using native's answer is the
        # whole point of the cutover.
        results = _select_results(
            books=[self._book()],
            portal=[self._book()],
            gstr1=[],
            graphowl_reconcile={"findings": []},
            tolerance=1.0,
        )
        assert results[0]["status"] == STATUS_MATCHED

    def test_graphowl_unreachable_falls_back_to_reconciliation_py(self):
        # Best-effort, matching every other graph-owl integration point in
        # this file: a laptop with no graph-owl running must not break the
        # app, so an unreachable native engine degrades to the old
        # Python-side math rather than returning nothing at all.
        results = _select_results(
            books=[self._book()],
            portal=[self._book()],
            gstr1=[],
            graphowl_reconcile={"error": "connection refused"},
            tolerance=1.0,
        )
        assert results[0]["status"] == STATUS_MATCHED
        assert len(results) == 1


class TestIngestUsesAStableSourceReplacedOnEveryUpload:
    """Plan 120 Slice D — confirmed root cause of totals that grew across
    every upload a session ever made: `_ingest_to_graphowl` minted a fresh
    random source per call, so a re-upload never replaced the last one, it
    only added a new named graph beside it. Now: one stable source per
    kind, deleted immediately before each import."""

    def _dataset(self) -> dict:
        return {
            "headers": ["Invoice No", "GSTIN"],
            "rows": [{"Invoice No": "INV-1", "GSTIN": "27AAAFN2938K1Z2"}],
        }

    def _mapping(self) -> dict:
        return {"invoice_no": 0, "supplier_gstin": 1}

    def test_deletes_the_stable_source_before_importing_it(self, monkeypatch):
        calls: list[tuple[str, str]] = []
        monkeypatch.setattr(
            graphowl_client,
            "delete_document",
            lambda server, source, token=None: calls.append(("delete", source)) or {"deleted": 0},
        )
        monkeypatch.setattr(
            graphowl_client,
            "import_document",
            lambda server, source, turtle, token=None: calls.append(("import", source))
            or {"landed": [], "skipped": [], "rejected": []},
        )
        main.SESSION["graphowl"] = {}

        thread = _ingest_to_graphowl("books", self._dataset(), self._mapping())
        thread.join()

        assert calls == [("delete", "reco-books"), ("import", "reco-books")], (
            "delete must run, and against the same source import then uses, "
            f"before the import: {calls}"
        )

    def test_the_source_is_the_same_across_repeated_uploads_of_the_same_kind(self, monkeypatch):
        sources: list[str] = []
        monkeypatch.setattr(
            graphowl_client,
            "delete_document",
            lambda server, source, token=None: {"deleted": 0},
        )
        monkeypatch.setattr(
            graphowl_client,
            "import_document",
            lambda server, source, turtle, token=None: sources.append(source)
            or {"landed": [], "skipped": [], "rejected": []},
        )
        main.SESSION["graphowl"] = {}

        _ingest_to_graphowl("books", self._dataset(), self._mapping()).join()
        _ingest_to_graphowl("books", self._dataset(), self._mapping()).join()

        assert sources == ["reco-books", "reco-books"], (
            "a random suffix here is exactly the bug this slice fixes — two "
            f"uploads of the same kind must land in the same graph: {sources}"
        )


class TestInstallGraphowlPackImportsTheOntology:
    """Plan 120 Slice A — `load_pack(..., include_documents=False)` skips
    every `[[documents]]` entry in packs/gst/pack.toml, and `ontology.ttl`
    (source `gst-ontology`) is declared there, right alongside
    `law/sections.ttl`/`law/rule-36-4.ttl`, which the surrounding code
    already knows to import directly for the same reason. Missing this one
    means the Ontology Builder's "gst" selector loads against a deployment
    that was never given the ontology, and reads as a permanently broken
    pack rather than a startup step that skipped a file."""

    def test_the_ontology_is_imported_the_same_way_the_law_data_already_is(self, monkeypatch):
        monkeypatch.setattr(main, "load_pack", lambda *a, **k: None)
        sources: list[str] = []
        monkeypatch.setattr(
            graphowl_client,
            "import_document",
            lambda server, source, text, token=None: sources.append(source) or {},
        )

        _install_graphowl_pack()

        assert "gst-ontology" in sources, (
            "ontology.ttl must be imported the same way law/sections.ttl and "
            f"law/rule-36-4.ttl already are: {sources}"
        )
