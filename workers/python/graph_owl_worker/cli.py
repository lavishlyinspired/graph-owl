"""``graph-owl-worker`` — point it at a directory and a catalog.

Follows the same stream discipline as the Rust CLI: results on stdout as JSON,
diagnostics on stderr, exit code says what happened. A worker is a scheduled
job far more often than it is a thing a human watches, so its output has to be
readable by the thing that scheduled it.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from graph_owl_sdk import GraphOwlClient, GraphOwlError

from .parsers import ParserRegistry
from .worker import Worker, catalog_subjects

#: 0 every document handled, 1 something failed, 2 the run could not start.
#: Distinguished because a scheduler retrying a 2 is right and retrying a 1 is
#: usually not — a corrupt PDF will still be corrupt in five minutes.
EXIT_OK = 0
EXIT_PARTIAL = 1
EXIT_UNUSABLE = 2


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-owl-worker",
        description="Parse documents and propose what they say to graph-owl.",
    )
    parser.add_argument("path", type=Path, help="a file or directory to process")
    parser.add_argument(
        "--server",
        default=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080"),
        help="graph-owl base URL (env: GRAPH_OWL_SERVER)",
    )
    parser.add_argument(
        "--token",
        default=os.environ.get("GRAPH_OWL_TOKEN"),
        help="bearer token (env: GRAPH_OWL_TOKEN)",
    )
    parser.add_argument(
        "--pdf",
        action="store_true",
        help="enable the PDF parser (needs the `pdf` extra installed)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    registry = ParserRegistry()
    if args.pdf:
        # Constructed only when asked for, so a worker handling markdown never
        # pays for a dependency it does not use — and one asked for PDF without
        # the extra installed is told exactly that, rather than failing later on
        # the first PDF it meets.
        from .parsers import PdfParser, UnsupportedMediaType

        try:
            registry.register(PdfParser())
        except UnsupportedMediaType as missing:
            print(str(missing), file=sys.stderr)
            return EXIT_UNUSABLE

    client = GraphOwlClient(args.server, token=args.token)
    try:
        subjects = catalog_subjects(client)
    except (GraphOwlError, OSError) as unreachable:
        # Failing here rather than per-document: with no subjects, *every* claim
        # would be discarded as being about an unknown entity, and the run would
        # report a tidy zero that looks like the documents said nothing.
        print(f"could not read the catalog: {unreachable}", file=sys.stderr)
        return EXIT_UNUSABLE

    worker = Worker(client, registry=registry)
    if args.path.is_dir():
        outcomes = worker.process_tree(args.path, subjects)
    else:
        outcomes = [worker.process(args.path, subjects)]

    for outcome in outcomes:
        json.dump(
            {
                "sourceId": outcome.source_id,
                "outcome": outcome.outcome,
                "detail": outcome.detail,
            },
            sys.stdout,
        )
        sys.stdout.write("\n")

    failed = [outcome for outcome in outcomes if outcome.outcome == "failed"]
    if failed:
        print(f"{len(failed)} of {len(outcomes)} documents failed", file=sys.stderr)
        return EXIT_PARTIAL
    return EXIT_OK


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
