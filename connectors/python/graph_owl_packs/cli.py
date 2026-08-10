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



def build_erpnext_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-owl-erpnext",
        description="Discover a Frappe/ERPNext DocType and land it in graph-owl.",
    )
    parser.add_argument(
        "doctype",
        nargs="+",
        help='DocType names, e.g. "Sales Invoice" "Item". Any DocType — the '
        "connector reads the schema from the instance and knows no domain.",
    )
    parser.add_argument(
        "--erpnext",
        default=os.environ.get("ERPNEXT_URL", "http://localhost:8080"),
        help="ERPNext base URL (env: ERPNEXT_URL)",
    )
    parser.add_argument("--api-key", default=os.environ.get("ERPNEXT_API_KEY"))
    parser.add_argument("--api-secret", default=os.environ.get("ERPNEXT_API_SECRET"))
    parser.add_argument(
        "--server",
        default=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8081"),
        help="graph-owl base URL (env: GRAPH_OWL_SERVER)",
    )
    parser.add_argument("--token", default=os.environ.get("GRAPH_OWL_TOKEN"))
    parser.add_argument("--limit", type=int, default=100, help="records per DocType")
    parser.add_argument("--dry-run", action="store_true")
    return parser


def erpnext_main(argv: list[str] | None = None) -> int:
    """`graph-owl-erpnext` — sync one or more DocTypes.

    **Each DocType is synced independently and a failure stops the run.** A
    partial sync reported as success is the worst outcome available: the
    operator sees a result and believes the estate is loaded.
    """
    from .erpnext import ErpnextClient, ErpnextError, sync_doctype
    from .loader import LoadError

    args = build_erpnext_parser().parse_args(argv)
    client = ErpnextClient(args.erpnext, args.api_key, args.api_secret)

    results = []
    for doctype in args.doctype:
        try:
            results.append(
                sync_doctype(
                    client,
                    doctype,
                    args.server,
                    token=args.token,
                    limit=args.limit,
                    dry_run=args.dry_run,
                )
            )
        except (ErpnextError, LoadError) as failed:
            print(f"{doctype}: {failed}", file=sys.stderr)
            return EXIT_UNUSABLE

    json.dump(
        {
            "doctypes": [
                {
                    "doctype": r.doctype,
                    "namespaceCode": r.namespace_code,
                    "predicates": r.predicates,
                    "landed": r.landed,
                    "skipped": r.skipped,
                }
                for r in results
            ],
            "landed": sum(r.landed for r in results),
            "skipped": sum(r.skipped for r in results),
        },
        sys.stdout,
    )
    sys.stdout.write("\n")
    return EXIT_OK


def build_reconcile_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-owl-reconcile",
        description="Run a pack's finding rules and queue what they conclude.",
    )
    parser.add_argument("pack", type=Path, help="the pack directory (containing pack.toml)")
    parser.add_argument(
        "--server",
        default=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080"),
        help="graph-owl base URL (env: GRAPH_OWL_SERVER)",
    )
    parser.add_argument("--token", default=os.environ.get("GRAPH_OWL_TOKEN"))
    return parser


def reconcile_main(argv: list[str] | None = None) -> int:
    """`graph-owl-reconcile` — evaluate a pack's rules.

    Exits `2` on an unevaluable rule rather than reporting a clean run: a
    reconciliation that silently found nothing is what somebody files a return
    on.
    """
    from .reconcile import ReconcileError, run_findings

    args = build_reconcile_parser().parse_args(argv)
    try:
        result = run_findings(args.pack, args.server, args.token)
    except (ManifestError, LoadError, ReconcileError) as failed:
        print(str(failed), file=sys.stderr)
        return EXIT_UNUSABLE

    json.dump(
        {
            "pack": result.pack_id,
            "rulesEvaluated": result.evaluated,
            "found": result.found,
            "opened": result.opened,
            "alreadyOpen": result.already_open,
        },
        sys.stdout,
    )
    sys.stdout.write("\n")
    return EXIT_OK


def build_gstr2b_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-owl-gstr2b",
        description="Normalize a GSTR-2B return into the GST pack's vocabulary.",
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--from-file", type=Path, help="a saved GSTR-2B JSON response")
    source.add_argument("--from-api", action="store_true", help="fetch from a GSP")
    parser.add_argument("--gstin", help="required with --from-api")
    parser.add_argument("--period", help="YYYY-MM; required with --from-api")
    parser.add_argument(
        "--gsp-url",
        default=os.environ.get("GSP_API_BASE_URL"),
        help="the GSP's base URL (env: GSP_API_BASE_URL) — no vendor is named "
        "in this tool, so changing provider is configuration",
    )
    parser.add_argument("--gsp-key", default=os.environ.get("GSP_API_KEY"))
    parser.add_argument("--out", type=Path, help="write Turtle here instead of stdout")
    return parser


def gstr2b_main(argv: list[str] | None = None) -> int:
    """`graph-owl-gstr2b` — the authority's JSON, in the pack's terms.

    Prints Turtle, so it composes with `graph-owl-load-pack` or with a plain
    `POST /graph/import/rdf`. Keeping the fetch and the load separate means a
    response can be inspected before anything lands in the graph.
    """
    from .gstr2b import Gstr2bError, fetch, from_file, normalize, to_turtle

    args = build_gstr2b_parser().parse_args(argv)
    try:
        if args.from_api:
            if not (args.gsp_url and args.gstin and args.period):
                print(
                    "--from-api needs --gsp-url (or GSP_API_BASE_URL), --gstin and --period",
                    file=sys.stderr,
                )
                return EXIT_UNUSABLE
            payload = fetch(args.gsp_url, args.gstin, args.period, api_key=args.gsp_key)
        else:
            payload = from_file(args.from_file)
        turtle = to_turtle(normalize(payload))
    except Gstr2bError as failed:
        print(str(failed), file=sys.stderr)
        return EXIT_UNUSABLE

    if args.out:
        args.out.write_text(turtle, encoding="utf-8")
        print(str(args.out))
    else:
        sys.stdout.write(turtle)
    return EXIT_OK


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
