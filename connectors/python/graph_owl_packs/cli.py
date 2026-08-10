"""`graph-owl-load-pack` — point it at a pack directory and a server.

Same stream discipline as the rest of this project's CLIs: results on stdout
as JSON, diagnostics on stderr, exit code says what happened. A pack load is a
scheduled step in a demo or a deployment far more often than something a human
watches.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from .loader import LoadError, load_pack
from .manifest import ManifestError

#: 0 loaded, 1 loaded with rejections, 2 could not load at all.
#: Distinguished for the same reason the document worker distinguishes them: a
#: scheduler retrying a 2 is right, and retrying a 1 usually is not — a shape
#: violation will still be a shape violation in five minutes.
EXIT_OK = 0
EXIT_PARTIAL = 1
EXIT_UNUSABLE = 2


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-owl-load-pack",
        description="Load a domain pack into graph-owl.",
    )
    parser.add_argument("pack", type=Path, help="the pack directory (containing pack.toml)")
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
        "--dry-run",
        action="store_true",
        help="parse and validate against the live shapes graph, writing nothing",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    try:
        result = load_pack(args.pack, args.server, args.token, args.dry_run)
    except (ManifestError, LoadError) as failed:
        print(str(failed), file=sys.stderr)
        return EXIT_UNUSABLE

    json.dump(
        {
            "pack": result.pack_id,
            "namespaceCode": result.namespace_code,
            "landed": result.landed,
            "skipped": result.skipped,
            "rejected": [{"subject": s, "reason": r} for s, r in result.rejected],
            "documents": [
                {
                    "source": d.source,
                    "landed": d.landed,
                    "skipped": d.skipped,
                    "rejected": len(d.rejected),
                }
                for d in result.documents
            ],
        },
        sys.stdout,
    )
    sys.stdout.write("\n")

    if result.rejected:
        print(
            f"{len(result.rejected)} subject(s) were rejected — the pack is "
            f"partially loaded",
            file=sys.stderr,
        )
        return EXIT_PARTIAL
    return EXIT_OK


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
