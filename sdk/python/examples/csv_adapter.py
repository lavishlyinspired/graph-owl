#!/usr/bin/env python3
"""A complete custom adapter, in one file — Epic 16 Slice F.

Reads a CSV of tables, derives the hierarchy they imply, and pushes it. This is
the reference for anybody writing an adapter for a source graph-owl will never
ship a connector for.

**It uses only the published SDK surface.** Nothing here imports a private
module or reaches into the client's internals, and a test asserts that — an
example that reaches into internals teaches people to reach into internals.

Run it:

    python csv_adapter.py --base-url http://localhost:8080 tables.csv
"""

from __future__ import annotations

import argparse
import csv
import sys
from typing import Iterable

from graph_owl_sdk import GraphOwlClient, IngestBuilder, IngestRequest


def build(rows: Iterable[dict[str, str]]) -> IngestRequest:
    """Turn flat rows into a hierarchy.

    A source almost never hands you the parents. This one gives
    ``service,database,schema,table`` per row, so every level above the table has
    to be *derived* — and derived **once**, because a batch that names the same
    FQN twice is refused rather than resolved (the batch would be stating two
    intents for one entity, and nothing can know which is meant).
    """
    builder = IngestBuilder()
    seen: set[str] = set()

    for row in rows:
        path: list[str] = []
        for level, kind in (
            ("service", "service"),
            ("database", "database"),
            ("schema", "schema"),
            ("table", "table"),
        ):
            name = (row.get(level) or "").strip()
            if not name:
                # A row missing a level cannot place anything below it. Skipped
                # rather than guessed at: an invented parent is a wrong fact, and
                # a wrong fact in a catalog outlives the adapter that wrote it.
                break
            parent = ".".join(path) if path else None
            path.append(name)
            fqn = ".".join(path)
            if fqn in seen:
                continue
            seen.add(fqn)
            builder.entity(
                kind,
                name,
                parent_fqn=parent,
                description=row.get("description") if kind == "table" else None,
            )

    return builder.build()


def report(result: dict) -> int:
    """Print what landed, and exit non-zero if anything did not.

    An adapter that always exits 0 is an adapter whose failures nobody notices —
    the scheduler running it every night is the only thing watching.
    """
    print(f"accepted {result['accepted']}, rejected {result['rejected']}")
    for item in result["results"]:
        if item["status"] >= 400:
            print(f"  item {item['index']}: {item.get('problem')}", file=sys.stderr)
    return 1 if result["rejected"] else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv_path")
    parser.add_argument("--base-url", default="http://localhost:8080")
    parser.add_argument("--token", default=None, help="a bot principal's token")
    args = parser.parse_args(argv)

    with open(args.csv_path, newline="", encoding="utf-8") as handle:
        request = build(csv.DictReader(handle))

    client = GraphOwlClient(base_url=args.base_url, token=args.token)
    # Batching, idempotency keys and retry-with-backoff are the SDK's job. An
    # adapter that reimplemented them would get the key discipline wrong, which
    # is the failure that duplicates an entire estate on one flaky night.
    return report(client.push(request))


if __name__ == "__main__":
    raise SystemExit(main())
