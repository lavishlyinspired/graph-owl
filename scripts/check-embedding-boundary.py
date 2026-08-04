#!/usr/bin/env python3
"""Enforce Epic 37c Slice A: `graph-owl-core` and `graph-owl-api` stay embeddable.

Three properties, each asserted rather than hoped for — "the same property
viewed from outside" only holds as long as nobody adds one dependency that
breaks it, and a check that only runs when someone remembers is not a check:

1. `graph-owl-core` depends on nothing but the pure, I/O-free crates it
   already uses (`37c-embeddable.md` decision 2) — no `graph-owl-*` crate,
   no database driver, no HTTP client, no async runtime.
2. `graph-owl-api` never depends on a *concrete* storage or search backend —
   only the `graph-owl-storage` port. Swapping Postgres for the in-memory
   backend must be a call-site change, not a recompile of `api` itself.
3. Neither crate constructs an async runtime, reads the process environment,
   or installs a global logger (decision 3) — an embedder brings their own
   executor and owns their own process-level state.

Scoped to `graph-owl-core` and `graph-owl-api` — the two crates this epic
makes embeddable — rather than every crate in the workspace. `graph-owl-server`
is the composition root and is expected to do all three; auditing the other
24 crates is a larger undertaking than this slice's own two named crates ask
for, and is a trigger for a wider check if a future epic needs it.

Run from the repository root; exits non-zero and names each offender.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

# What `graph-owl-core` actually needs: serialization, hashing, ids, time.
# Every one of these is pure computation — none does I/O or owns a runtime.
# Growing this list is fine; it is the *kind* of crate that must not change.
CORE_ALLOWED_DEPS = {
    "base64",
    "chrono",
    "serde",
    "serde_json",
    "sha2",
    "thiserror",
    "utoipa",
    "uuid",
}

# Concrete backend adapters `graph-owl-api` must reach only through the
# `graph-owl-storage` port, never by name — the whole point of the port.
# A new adapter crate (a Mongo or Elastic backend, say) belongs here too.
API_FORBIDDEN_DEPS = {
    "graph-owl-storage-postgres",
    "graph-owl-engine-postgres",
    "graph-owl-search-hnsw",
    "graph-owl-search-opensearch",
}

RUNTIME_PATTERNS = {
    "reads the process environment": re.compile(r"\bstd::env::var\b|(?<!std::)\benv::var\b"),
    "constructs a runtime": re.compile(r"\bRuntime::new\b|#\[tokio::main\]"),
    "installs a global logger": re.compile(r"\btracing_subscriber::|::set_global_default\b|\benv_logger::"),
}


def package_dependencies(metadata: dict, name: str) -> list[dict]:
    for pkg in metadata["packages"]:
        if pkg["name"] == name:
            return pkg["dependencies"]
    raise SystemExit(f"no such workspace package: {name}")


def check_core_deps(metadata: dict) -> list[str]:
    failures = []
    for dep in package_dependencies(metadata, "graph-owl-core"):
        if dep.get("kind") is not None:  # dev/build deps don't ship to an embedder
            continue
        if dep.get("path"):
            failures.append(f"graph-owl-core depends on {dep['name']}, another workspace crate")
        elif dep["name"] not in CORE_ALLOWED_DEPS:
            failures.append(f"graph-owl-core depends on {dep['name']}, not in the allowed I/O-free set")
    return failures


def check_api_adapters(metadata: dict) -> list[str]:
    failures = []
    for dep in package_dependencies(metadata, "graph-owl-api"):
        if dep.get("kind") is not None:
            continue
        if dep["name"] in API_FORBIDDEN_DEPS:
            failures.append(f"graph-owl-api depends directly on {dep['name']}, a concrete backend adapter")
    return failures


def check_runtime_hygiene(root: Path) -> list[str]:
    failures = []
    for crate in ("graph-owl-core", "graph-owl-api"):
        src = root / "crates" / crate / "src"
        for path in sorted(src.rglob("*.rs")):
            text = path.read_text(encoding="utf-8")
            for reason, pattern in RUNTIME_PATTERNS.items():
                if pattern.search(text):
                    failures.append(f"{path.relative_to(root)} {reason}")
    return failures


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=root,
            capture_output=True,
            check=True,
            text=True,
        ).stdout
    )

    failures = check_core_deps(metadata) + check_api_adapters(metadata) + check_runtime_hygiene(root)

    if not failures:
        print("embedding boundary: graph-owl-core and graph-owl-api stay embeddable")
        return 0

    print("Epic 37c's embedding boundary is broken:\n")
    for failure in failures:
        print(f"  {failure}")
    print(
        "\ngraph-owl-core must stay I/O-free (37c-embeddable.md decision 2) and\n"
        "graph-owl-api must reach every backend through the graph-owl-storage port\n"
        "rather than a concrete adapter. Neither crate may construct a runtime,\n"
        "read the environment, or install a global logger (decision 3) — an\n"
        "embedder owns all three."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
