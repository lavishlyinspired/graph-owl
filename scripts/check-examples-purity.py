#!/usr/bin/env python3
"""Enforce Epic 36 Slice A: every reference app depends only on published
surfaces — the standard library, `graph_owl_sdk`'s public exports, and
`graph_owl_read_client`'s public exports (Slice D's generated read client).

The plan's own wording ("no pub(crate) reach-through... build against the
*published* crate versions, not workspace paths") is Rust-flavored, written
before `00j-language-boundaries.md` settled reference applications as
Python (`00j`: "examples should look like what a user would actually
write"). Translated: an example must never reach past a published
package's own `__init__.py`-declared exports into its private submodules,
never manipulate `sys.path` to read source directly, and never import
anything that is not the standard library or one of the two SDK packages
this repository publishes for Python.

Checked with `ast`, not a regex over import lines — a regex cannot tell
`from graph_owl_sdk import GraphOwlClient` (public, allowed) from
`from graph_owl_sdk.ingest import GraphOwlClient` (the same symbol, but
reached through a private submodule, not allowed) without parsing the
statement's actual structure.

Run from the repository root; exits non-zero and names each offender.
"""

from __future__ import annotations

import ast
import importlib.util
import sys
from pathlib import Path

# The two SDK packages this repository publishes for Python. An example may
# `from <package> import <name>` only if `<name>` is one of that package's
# own declared public exports — never reach into a submodule directly.
PUBLISHED_PACKAGES = {
    "graph_owl_sdk",
    "graph_owl_read_client",
}

# `sys.path` manipulation is exactly how an example would reach past a
# published package's install boundary into the monorepo's own source —
# the one thing "published crate versions, not workspace paths" forbids,
# translated to Python.
FORBIDDEN_CALLS = {
    ("sys.path", "insert"),
    ("sys.path", "append"),
}


def public_exports(package: str) -> set[str] | None:
    """The names `package.__all__` declares, or `None` if the package
    cannot even be imported (found only by trying — a stale allowlist
    entry for a package that no longer builds is worse than silence)."""
    spec = importlib.util.find_spec(package)
    if spec is None:
        return None
    module = importlib.import_module(package)
    return set(getattr(module, "__all__", []))


def stdlib_names() -> set[str]:
    if sys.version_info >= (3, 10):
        return set(sys.stdlib_module_names)
    # No `sys.stdlib_module_names` before 3.10 — the check still needs to
    # run somewhere, so fall back to what is actually importable from a
    # bare interpreter rather than refuse to check anything at all.
    return set(sys.builtin_module_names)


def local_sibling_modules(path: Path) -> set[str]:
    """Other `.py` files in this example's own directory — the app's own
    code, not a package boundary. `triage.py` importing `mcp_client.py`
    from the same `examples/agent-triage/` directory is the app being one
    app in two files, not reaching outside the published-surfaces
    boundary Slice A actually exists to enforce."""
    return {sibling.stem for sibling in path.parent.glob("*.py") if sibling != path}


def check_file(path: Path, exports: dict[str, set[str] | None], stdlib: set[str]) -> list[str]:
    failures: list[str] = []
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    local_modules = local_sibling_modules(path)

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                top = alias.name.split(".")[0]
                if top in stdlib or top in PUBLISHED_PACKAGES or top in local_modules:
                    continue
                failures.append(f"{path}:{node.lineno}: imports {alias.name!r}, not stdlib or a published package")

        elif isinstance(node, ast.ImportFrom):
            module = node.module or ""
            top = module.split(".")[0]
            if node.level > 0 or top in local_modules:
                # A relative import (`from . import x`) or a bare import of
                # a sibling file in this same example's own directory.
                continue
            if top in stdlib:
                continue
            if top in PUBLISHED_PACKAGES:
                if module != top:
                    failures.append(
                        f"{path}:{node.lineno}: `from {module} import ...` reaches into a "
                        f"private submodule of {top!r} — import from {top!r} itself"
                    )
                    continue
                allowed = exports.get(top)
                if allowed is None:
                    failures.append(f"{path}:{node.lineno}: {top!r} is not importable — is it installed?")
                    continue
                for alias in node.names:
                    if alias.name not in allowed:
                        failures.append(
                            f"{path}:{node.lineno}: `{alias.name}` is not one of {top}'s "
                            f"declared public exports ({sorted(allowed)})"
                        )
            else:
                failures.append(f"{path}:{node.lineno}: imports from {module!r}, not stdlib or a published package")

        elif isinstance(node, ast.Call):
            attr_chain = _attribute_chain(node.func)
            if attr_chain and (".".join(attr_chain[:-1]), attr_chain[-1]) in FORBIDDEN_CALLS:
                failures.append(
                    f"{path}:{node.lineno}: calls {'.'.join(attr_chain)}(...) — reaching outside "
                    "the installed package boundary, exactly what a published-surfaces-only "
                    "example must not do"
                )

    return failures


def _attribute_chain(node: ast.expr) -> list[str] | None:
    """`sys.path.insert` -> `["sys", "path", "insert"]`, or `None` for
    anything that is not a plain dotted attribute chain."""
    parts: list[str] = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if isinstance(node, ast.Name):
        parts.append(node.id)
        return list(reversed(parts))
    return None


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    examples_dir = root / "examples"
    if not examples_dir.is_dir():
        print("no examples/ directory — nothing to check")
        return 0

    stdlib = stdlib_names()
    exports = {pkg: public_exports(pkg) for pkg in PUBLISHED_PACKAGES}

    # Test harnesses are scaffolding, not the application: proving an
    # example works legitimately needs things the app itself must not do
    # (mint a test JWT, call an admin endpoint directly to seed a scenario).
    # `conftest.py` is pytest's own fixture-discovery convention.
    failures: list[str] = []
    for path in sorted(examples_dir.rglob("*.py")):
        if path.name.startswith("test_") or path.name == "conftest.py":
            continue
        failures.extend(check_file(path, exports, stdlib))

    if not failures:
        print("surface purity: every example imports only the standard library and published SDK exports")
        return 0

    print("Epic 36 Slice A's surface-purity check failed:\n")
    for failure in failures:
        print(f"  {failure}")
    print(
        "\nA reference application under examples/ may import only the standard "
        "library and the public exports of graph_owl_sdk / graph_owl_read_client "
        "— the two packages this repository publishes for Python. Friction that "
        "makes this hard is a defect in graph-owl, not the example "
        "(36-reference-apps.md decision 2) — fix the SDK, don't work around it here."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
