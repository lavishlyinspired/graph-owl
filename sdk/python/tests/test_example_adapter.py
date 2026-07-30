"""The example adapter runs, and only touches published surface — Epic 16 Slice F.

Two things are asserted here, and the second is the one that decays without a
test: an example that reaches into internals teaches everybody reading it to
reach into internals, and it does so silently.
"""

from __future__ import annotations

import ast
import csv
import importlib.util
import io
import json
import sys
from pathlib import Path
from typing import Any

import graph_owl_sdk

EXAMPLES = Path(__file__).resolve().parents[1] / "examples"
ADAPTER = EXAMPLES / "csv_adapter.py"


def load_adapter() -> Any:
    spec = importlib.util.spec_from_file_location("csv_adapter", ADAPTER)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules["csv_adapter"] = module
    spec.loader.exec_module(module)
    return module


def fixture_rows() -> list[dict[str, str]]:
    with open(EXAMPLES / "tables.csv", newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def test_the_example_derives_every_level_of_the_hierarchy() -> None:
    request = load_adapter().build(fixture_rows())

    fqns = []
    for item in request.items:
        parent = item.parent_fqn
        fqns.append(f"{parent}.{item.name}" if parent else item.name)

    assert "payments" in fqns
    assert "payments.core.public.orders" in fqns
    assert "billing.ledger.public.invoices" in fqns


def test_a_shared_parent_is_declared_once() -> None:
    """A batch naming one FQN twice is refused, not merged — the batch would be
    stating two intents for one entity and nothing can know which is meant."""
    request = load_adapter().build(fixture_rows())

    fqns = [
        f"{i.parent_fqn}.{i.name}" if i.parent_fqn else i.name for i in request.items
    ]
    assert len(fqns) == len(set(fqns)), fqns


def test_a_row_missing_a_level_places_nothing_below_it() -> None:
    """An invented parent is a wrong fact, and a wrong fact in a catalog outlives
    the adapter that wrote it."""
    request = load_adapter().build(
        [{"service": "svc", "database": "", "schema": "public", "table": "orders"}]
    )

    assert [item.name for item in request.items] == ["svc"]


def test_the_example_exits_non_zero_when_anything_was_rejected() -> None:
    """An adapter that always exits 0 is one whose failures nobody notices — the
    scheduler running it nightly is the only thing watching."""
    adapter = load_adapter()

    clean = {"accepted": 2, "rejected": 0, "results": []}
    dirty = {
        "accepted": 1,
        "rejected": 1,
        "results": [{"index": 1, "status": 400, "problem": "no"}],
    }

    assert adapter.report(clean) == 0
    assert adapter.report(dirty) == 1


def test_the_example_pushes_through_the_sdk_end_to_end(monkeypatch) -> None:
    """The whole path, against a stub server: argument parsing, CSV reading,
    hierarchy derivation, and the SDK's own batching and key handling."""
    adapter = load_adapter()
    seen: list[dict[str, Any]] = []

    class Response:
        status = 207

        def read(self) -> bytes:
            return json.dumps({"accepted": 8, "rejected": 0, "results": []}).encode()

        def __enter__(self) -> "Response":
            return self

        def __exit__(self, *_: object) -> None:
            return None

    def opener(request: Any) -> Response:
        seen.append(json.loads(request.data))
        return Response()

    real_client = graph_owl_sdk.GraphOwlClient
    monkeypatch.setattr(
        adapter,
        "GraphOwlClient",
        lambda **kwargs: real_client(opener=opener, sleep=lambda _: None, **kwargs),
    )

    exit_code = adapter.main([str(EXAMPLES / "tables.csv"), "--base-url", "http://x"])

    assert exit_code == 0
    assert len(seen) == 1
    assert any(item["name"] == "orders" for item in seen[0]["items"])


def test_the_example_imports_only_published_sdk_surface() -> None:
    """Read as source rather than by import, because an import that reached into
    a private module would still succeed — Python has no access control, so the
    only way to check this is to look."""
    tree = ast.parse(ADAPTER.read_text(encoding="utf-8"))

    sdk_imports: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and (node.module or "").startswith(
            "graph_owl_sdk"
        ):
            assert node.module == "graph_owl_sdk", (
                f"the example reaches into `{node.module}`; an example that uses "
                "internals teaches everybody reading it to use internals"
            )
            sdk_imports.extend(alias.name for alias in node.names)
        if isinstance(node, ast.Import):
            for alias in node.names:
                assert alias.name == "graph_owl_sdk" or not alias.name.startswith(
                    "graph_owl_sdk."
                ), f"the example imports `{alias.name}`, which is not published"

    assert sdk_imports, "the example is supposed to demonstrate the SDK"
    for name in sdk_imports:
        assert name in graph_owl_sdk.__all__, (
            f"`{name}` is used by the example but is not in `__all__` — either "
            "publish it or stop using it"
        )


def test_the_example_writes_its_report_where_a_scheduler_will_see_it(capsys) -> None:
    adapter = load_adapter()
    buffer = io.StringIO()
    sys.stdout = buffer
    try:
        adapter.report({"accepted": 1, "rejected": 0, "results": []})
    finally:
        sys.stdout = sys.__stdout__

    assert "accepted 1" in buffer.getvalue()
