#!/usr/bin/env python3
"""Regenerate `plans/_COMPILED_INDEX.md` from the plans themselves.

The previous index was hand-written and drifted catastrophically: 60 of the 86
files it named did not exist, and it encoded a *different* epic numbering
(`08-authorization.md` when Epic 8 is Search, `16-search.md` when Epic 16 is
ingestion). An index that misroutes is worse than no index, because it is
consulted instead of `ls`.

So it is generated. Run this after adding or restatusing a plan:

    python3 scripts/compile-plan-index.py

Every column comes from the plan's own header — `**Status**`, `**Depends on**`,
`**Unblocks**` — so the index cannot disagree with the file it points at. It
does not invent a status: a plan without one is reported as missing a header,
which is a finding rather than a blank cell.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLANS = ROOT / "plans"

# Standing reference documents, which have no slices and no dependencies.
STANDING = "00"


def field(head: str, name: str) -> str:
    match = re.search(
        rf"^\*\*{name}\*\*:\s*(.+?)(?=\n\*\*|\n##|\n\n|\Z)", head, re.M | re.S
    )
    return " ".join(match.group(1).split()) if match else ""


def cell(text: str, limit: int = 150) -> str:
    text = text.replace("|", "\\|").strip()
    if not text:
        return "—"
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"


def read(path: pathlib.Path) -> dict:
    text = path.read_text()
    head = text.split("\n## ", 1)[0]
    title = ""
    first = re.match(r"#\s*(.+)", text)
    if first:
        title = first.group(1).strip()
    return {
        "file": path.name,
        "title": title,
        "status": field(head, "Status"),
        "depends": field(head, "Depends on"),
        "unblocks": field(head, "Unblocks"),
    }


def main() -> int:
    plans = sorted(p for p in PLANS.glob("*.md") if not p.name.startswith("_"))
    entries = [read(p) for p in plans]

    standing = [e for e in entries if e["file"].startswith(STANDING)]
    special = [e for e in entries if e["file"] in {"ROADMAP.md", "DEMOS.md"}]
    done = [e for e in entries if e["file"].startswith("9") and "-done-" in e["file"]]
    named = {e["file"] for e in standing + special + done}
    epics = [e for e in entries if e["file"] not in named]

    out = [
        "# Plan Index",
        "",
        "> **Generated** by `scripts/compile-plan-index.py`. Do not hand-edit —",
        "> every column is read from the plan's own `**Status**` /",
        "> `**Depends on**` / `**Unblocks**` header, so the index cannot",
        "> disagree with the file it points at. Re-run it after adding or",
        "> restatusing a plan.",
        "",
        "Rebuilt 28 July 2026, replacing a hand-written index in which **60 of",
        "the 86 files named did not exist** and the epic numbering was a",
        "different scheme entirely (`08-authorization.md` when Epic 8 is",
        "Search; `16-search.md` when Epic 16 is ingestion). An index that",
        "misroutes is worse than none, because it gets consulted instead of",
        "`ls`.",
        "",
        f"**{len(entries)} plan documents.** `DEMOS.md` remains the authority on",
        "what is *built* — this file is the authority on what *exists* and how",
        "the documents relate.",
        "",
        "## Standing reference",
        "",
        "| File | Role |",
        "|---|---|",
    ]
    for e in standing:
        out.append(f"| [`{e['file']}`]({e['file']}) | {cell(e['title'], 110)} |")

    out += [
        "",
        "## Sequencing",
        "",
        "| File | Role |",
        "|---|---|",
    ]
    for e in special:
        out.append(f"| [`{e['file']}`]({e['file']}) | {cell(e['title'], 110)} |")

    out += [
        "",
        "## Epic plans",
        "",
        "| File | Epic | Status | Depends on |",
        "|---|---|---|---|",
    ]
    for e in epics:
        epic = re.search(r"\(Epics? ([0-9a-z,\s–-]+)\)", e["title"])
        out.append(
            f"| [`{e['file']}`]({e['file']}) | {epic.group(1) if epic else '—'} "
            f"| {cell(e['status'])} | {cell(e['depends'], 90)} |"
        )

    out += [
        "",
        "## Completed, kept as record",
        "",
        "| File | Status |",
        "|---|---|",
    ]
    for e in done:
        out.append(f"| [`{e['file']}`]({e['file']}) | {cell(e['status'], 110)} |")

    missing = [e["file"] for e in epics if not e["status"]]
    out += [
        "",
        "## Plans with no `**Status**` header",
        "",
    ]
    out.append(
        ", ".join(f"`{m}`" for m in missing)
        if missing
        else "None — every epic plan states its status."
    )
    out.append("")

    (PLANS / "_COMPILED_INDEX.md").write_text("\n".join(out))
    print(f"wrote plans/_COMPILED_INDEX.md — {len(entries)} documents, {len(missing)} missing a status")
    return 0


if __name__ == "__main__":
    sys.exit(main())
