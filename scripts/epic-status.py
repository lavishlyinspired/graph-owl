#!/usr/bin/env python3
"""Regenerate `plans/EPIC-STATUS.md`: one row per epic, its slices, dependencies.

Replaces `_COMPILED_INDEX.md`, deleted 29 July 2026. That file was hand-written,
named 86 plan files of which 60 did not exist, and encoded a different epic
numbering entirely (`08-authorization.md` when Epic 8 is Search). An index that
misroutes is worse than none, because it is consulted instead of `ls`.

Two sources, each authoritative for one thing and neither guessed at:

  DEMOS.md   what is *built* — the `[x]` / `[~]` / `[ ]` slice marks (rule 0)
  plans/*.md what *exists* — the `**Depends on**` header of each plan

Run after changing either:

    python3 scripts/epic-status.py
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLANS = ROOT / "plans"

DONE, PARTIAL, TODO = "[x]", "[~]", "[ ]"


def epic_numbers(title: str) -> list[str]:
    """`(Epics 12–13)` → ['12', '13']; `(Epic 7a) ★` → ['7a']."""
    match = re.search(r"\(Epics?\s+([^)]+)\)", title)
    if not match:
        return []
    return re.findall(r"\d+[a-z]?", match.group(1))


def plans_by_epic() -> dict[str, dict]:
    """Every epic number mapped to the plan that owns it."""
    owners: dict[str, dict] = {}
    for path in sorted(PLANS.glob("*.md")):
        if path.name.startswith(("_", "00")) or path.name in {"ROADMAP.md", "DEMOS.md"}:
            continue
        text = path.read_text()
        head = text.split("\n## ", 1)[0]
        first = re.match(r"#\s*(.+)", text)
        title = first.group(1).strip() if first else path.name
        depends = re.search(
            r"^\*\*Depends on\*\*:\s*(.+?)(?=\n\*\*|\n##|\n\n|\Z)", head, re.M | re.S
        )
        for number in epic_numbers(title):
            owners[number] = {
                "file": path.name,
                "title": re.sub(r"^Plan:\s*", "", title),
                "depends": " ".join(depends.group(1).split()) if depends else "",
            }
    return owners


def slices_by_epic() -> dict[str, list[tuple[str, str]]]:
    """Slice marks from DEMOS.md, grouped under the `### Epic N` they sit below.

    DEMOS is the authority on what is built (its rule 0), so the marks are read
    rather than restated. An epic appearing under several demos accumulates —
    Epic 39 has lines under Demo 1 and Demo 2, and both are its state.
    """
    marks: dict[str, list[tuple[str, str]]] = {}
    current: list[str] = []
    # DEMOS lists unfinished work two ways: as `[ ]` marks, and as plain
    # bullets under a **Pending in this epic** heading. Counting only the
    # first reports Epic 10 as "9/9 shipped" while the file says four things
    # are outstanding — an index that overstates completion, which is the
    # failure the old one had. Bullets under that heading count as open.
    pending_block = False
    for line in (PLANS / "DEMOS.md").read_text().splitlines():
        # The terminator requires *surrounding* whitespace so it only matches
        # a title separator (" — Description", spaced both sides) and not a
        # bare numeric range like "25–30" or "37a–c", where the dash sits
        # tight against its digits. The lazy capture group used to stop at
        # the first dash of either kind, so "Epics 22, 23, 25–30" read as
        # epics 22, 23 and 25 only — 26 through 30 silently vanished from
        # this heading's marks. Found while Epic 24 restructured this exact
        # heading and the epics after it turned up with no marks at all.
        heading = re.match(
            r"^###\s+Epics?\s+([0-9a-z,\s/–—-]+?)(?:\s+[—–]\s|\s*$)", line
        )
        if heading:
            current = re.findall(r"\d+[a-z]?", heading.group(1))
            pending_block = False
            continue
        if line.startswith("### ") or line.startswith("## "):
            current = []
            pending_block = False
            continue
        if re.match(r"^\*\*Pending in this epic\*\*", line):
            pending_block = True
            continue
        if re.match(r"^\*\*(Deferred|Decision|Found|The demo moment|What)", line):
            pending_block = False
            continue

        if pending_block and current:
            plain = re.match(r"^- (?!\[[x~ ]\])(.+)", line)
            if plain:
                summary = plain.group(1)
                label = re.match(r"\*\*(.+?)\*\*", summary)
                summary = label.group(1) if label else summary.split(".")[0]
                for number in current:
                    marks.setdefault(number, []).append((TODO, summary[:88]))
                continue

        mark = re.match(r"^- (\[[x~ ]\])\s*(.+)", line)
        if mark and current:
            text = mark.group(2)
            label = re.match(r"\*\*(.+?)\*\*\s*(.*)", text)
            bold = label.group(1) if label else None
            rest = label.group(2) if label else ""

            # Under a heading naming several epics (`### Epics 7b, 7c, 7d, 9,
            # 9a`), each checkbox names its own in bold at the start
            # (`**7d** Bolt server: ...`) precisely so a mark can be
            # attributed to the one epic it is actually about — not every
            # epic the heading covers. Skipping this was the bug: all five
            # epics under that heading read identically, each showing every
            # other epic's one-line marks as if they were its own, because
            # the loop below filed every checkbox under every `current`
            # epic regardless of which one its own bold prefix named.
            if bold and bold in current:
                summary = rest.strip(" —-:.") or bold
                marks.setdefault(bold, []).append((mark.group(1), summary[:88]))
                continue

            # A short bold run that is *not* an epic number is a label, not
            # a summary — a slice letter (`**A**`, `**B2**`). Several
            # bullets sharing one slice letter would otherwise all read as
            # the single word "A"; the real summary is what follows it.
            if bold and rest and re.fullmatch(r"[A-Z][0-9]?", bold):
                summary = rest.strip(" —-:.")
            elif bold:
                summary = bold
            else:
                summary = text.split(".")[0]
            for number in current:
                marks.setdefault(number, []).append((mark.group(1), summary[:88]))
    return marks


def demos_by_epic() -> dict[str, list[str]]:
    """Which demo each epic serves, from DEMOS.md's own coverage index.

    Read rather than restated, for the same reason the slice marks are: the
    index is maintained beside the demos it describes, and a second copy here
    is a second thing to keep in step. An epic may serve more than one demo —
    Epic 6 is Demo 4's reasoning and is recalibrated in Demo 12 — so this is a
    list, and a single number would quietly drop the later work.
    """
    demos: dict[str, list[str]] = {}
    in_table = False
    for line in (PLANS / "DEMOS.md").read_text().splitlines():
        if re.match(r"^\|\s*Demo\s*\|\s*Epics covered\s*\|", line):
            in_table = True
            continue
        if in_table:
            row = re.match(r"^\|\s*(\d+)\s*★?\s*\|\s*(.+?)\s*\|\s*$", line)
            if not row:
                # The first non-row ends the table. Reading past it would pick
                # up whatever prose follows as epic numbers.
                if not line.startswith("|"):
                    break
                continue
            demo, covered = row.group(1), row.group(2)
            for number in re.findall(r"\d+[a-z]?", covered):
                demos.setdefault(number, []).append(demo)
    return demos


def bar(slices: list[tuple[str, str]]) -> str:
    if not slices:
        return "—"
    done = sum(1 for m, _ in slices if m == DONE)
    partial = sum(1 for m, _ in slices if m == PARTIAL)
    return f"{done}/{len(slices)}" + (f" (+{partial} partial)" if partial else "")


def state(slices: list[tuple[str, str]]) -> str:
    if not slices:
        return "Not started"
    marks = [m for m, _ in slices]
    if all(m == DONE for m in marks):
        return "**Shipped**"
    if any(m in (DONE, PARTIAL) for m in marks):
        return "In progress"
    return "Not started"


def main() -> int:
    owners = plans_by_epic()
    marks = slices_by_epic()
    demos = demos_by_epic()

    def order(number: str) -> tuple[int, str]:
        digits = re.match(r"(\d+)", number)
        return (int(digits.group(1)) if digits else 0, number)

    numbers = sorted(set(owners) | set(marks), key=order)

    out = [
        "# Epic status",
        "",
        "> **Generated** by `scripts/epic-status.py`. Do not hand-edit.",
        ">",
        "> Slice marks come from `DEMOS.md`, which is the authority on what is",
        "> built (its rule 0). Dependencies come from each plan's own",
        "> `**Depends on**` header. Nothing here is restated by hand, so this",
        "> file cannot disagree with either source.",
        "",
        "Replaces `_COMPILED_INDEX.md`, deleted 29 July 2026: it named 86 plan",
        "files of which **60 did not exist**, and used a different epic",
        "numbering (`08-authorization.md` when Epic 8 is Search).",
        "",
        "**Tracked items are not the plan\'s slices, and the difference matters.**",
        "This column counts `DEMOS.md` checkboxes *plus* the bullets under a",
        "**Pending in this epic** heading — which is deliberate, so an epic",
        "cannot read complete while the file lists outstanding work. The",
        "consequence is that the ratio **understates** an epic whose remaining",
        "work is itemised: Epic 31 shows 2/5 while all five of its plan slices",
        "(A–E) have domain logic, persistence and an HTTP surface, because two",
        "of those five items are *pending notes* rather than slices. Read a low",
        "ratio as \"outstanding work is written down\", not as \"slices unwritten\",",
        "and read the plan for slice-level state.",
        "",
        "`—` means the plan exists and `DEMOS.md` tracks no marks for it yet,",
        "which is a different thing from zero of them done.",
        "",
        "**Demo** is which demo an epic serves, from `DEMOS.md`'s coverage",
        "index. An epic serving more than one shows both — Epic 6 is Demo 4's",
        "reasoning *and* is recalibrated in Demo 12, and a single number would",
        "quietly drop the later work. `—` means the epic is in no demo, which",
        "is the condition that index exists to catch.",
        "",
        "| Epic | Demo | Plan | State | Tracked items | Depends on |",
        "|---|---|---|---|---|---|",
    ]
    for number in numbers:
        plan = owners.get(number, {})
        slices = marks.get(number, [])
        file = plan.get("file", "")
        link = f"[`{file}`]({file})" if file else "**no plan file**"
        depends = plan.get("depends", "") or "—"
        if len(depends) > 80:
            depends = depends[:79].rstrip() + "…"
        demo = ", ".join(demos.get(number, [])) or "—"
        out.append(
            f"| **{number}** | {demo} | {link} | {state(slices)} | {bar(slices)} "
            f"| {depends.replace('|', chr(92) + '|')} |"
        )

    out += ["", "## Tracked items, per epic", ""]
    for number in numbers:
        slices = marks.get(number, [])
        if not slices:
            continue
        plan = owners.get(number, {})
        title = re.sub(r"\s*\(Epics?[^)]*\)\s*", " ", plan.get("title", "no plan file")).strip()
        demo = ", ".join(demos.get(number, []))
        where = f" *(Demo {demo})*" if demo else ""
        out.append(f"### Epic {number} — {title}{where}")
        out.append("")
        for mark, summary in slices:
            out.append(f"- {mark} {summary}")
        out.append("")

    orphans = [n for n in numbers if n not in owners]
    out += ["## Epics with slice marks and no plan file", ""]
    out.append(
        ", ".join(f"**{n}**" for n in orphans)
        if orphans
        else "None — every epic tracked in `DEMOS.md` has a plan."
    )
    out.append("")

    (PLANS / "EPIC-STATUS.md").write_text("\n".join(out))
    print(f"wrote plans/EPIC-STATUS.md — {len(numbers)} epics, {len(orphans)} without a plan")
    return 0


if __name__ == "__main__":
    sys.exit(main())
