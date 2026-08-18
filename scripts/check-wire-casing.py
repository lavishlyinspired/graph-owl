#!/usr/bin/env python3
"""Refuse a serde-tagged enum that ships snake_case fields on a camelCase wire.

**This bug has shipped five times**, in five different epics:

    Authorship.agent_id          Epic 18
    SubmissionOutcome.run_id     Epic 21
    AssetListQuery.data_product  Epic 23   (a query struct, not an enum)
    CertificationStatus.days_remaining   Epic 26
    Sla.max_age_seconds          Epic 27

Every time, the cause is the same and it is not carelessness: `#[serde(rename_all
= "camelCase")]` on an **enum** renames the *variants*, not the fields inside
them. And every time it survived review because most variants have single-word
fields — `column`, `detail`, `observed`, `id` — which are immune by luck. Only a
multi-word field exposes it, so a test that happens to pick the wrong variant
proves nothing.

**The fix is a per-field `#[serde(rename = "...")]`, not the enum-level
`rename_all_fields`.** Serde honours both; utoipa 5's schema derive reads only
the per-field form. So `rename_all_fields` on a type that derives `ToSchema`
produces correct JSON beside an OpenAPI schema that still says `days_remaining`
— the two disagree, and every generated client is built against the wrong one.
That is worse than either being wrong alone, and it is how this check came to
exist: the Epic 26 "fix" used `rename_all_fields` and was itself wrong.

Run from the repository root; exits non-zero and names each offender.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# A `#[serde(...)]` attribute block immediately followed by `pub enum Name {`,
# capturing the attribute text, the name, and the body up to the enum's closing
# brace at column zero.
ENUM = re.compile(
    r"#\[serde\(([^)]*)\)\]\s*\n\s*pub enum (\w+)\s*\{(.*?)\n\}", re.S
)
# A field inside a struct-variant's braces whose name has an underscore.
BRACED = re.compile(r"\{([^{}]*)\}", re.S)
FIELD = re.compile(r"^\s*(?:pub )?(\w+_\w+)\s*:", re.M)
# A per-field rename immediately above it is the sanctioned fix.
RENAMED = re.compile(r'#\[serde\(rename\s*=\s*"[^"]+"\)\]\s*\n\s*(?:pub )?(\w+_\w+)\s*:')
# An actual `#[derive(... ToSchema ...)]` attribute — not just the word
# "ToSchema" anywhere in the preceding text, which a doc comment explaining
# why the type does *not* derive it (see `RdfEditDryRun`) would also match.
DERIVES_TO_SCHEMA = re.compile(r"#\[derive\([^)]*\bToSchema\b[^)]*\)\]")


def offenders(path: Path) -> list[tuple[str, list[str]]]:
    source = path.read_text(encoding="utf-8")
    found = []
    for attrs, name, body in ENUM.findall(source):
        # Only tagged enums: an untagged or externally-tagged one nests its
        # fields under the variant name, where `rename_all` does reach them.
        if "tag =" not in attrs:
            continue
        # Only a camelCase wire can have a snake_case/camelCase mismatch at
        # all — a `rename_all = "snake_case"` enum (pack-config deserializing
        # types, for instance) has no camelCase wire to disagree with.
        if "camelCase" not in attrs:
            continue
        renamed = set(RENAMED.findall(body))
        exposed = sorted(
            {
                field
                for block in BRACED.findall(body)
                for field in FIELD.findall(block)
                if field not in renamed
            }
        )
        # `rename_all_fields` fixes serde but not utoipa, so it only counts as a
        # fix for a type that does not derive `ToSchema`.
        preceding = source[: source.index(f"pub enum {name}")]
        derives_to_schema = bool(DERIVES_TO_SCHEMA.search(preceding[-800:]))
        if exposed and not ("rename_all_fields" in attrs and not derives_to_schema):
            found.append((name, exposed))
    return found


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    failures = []
    for path in sorted((root / "crates").rglob("*.rs")):
        for name, fields in offenders(path):
            failures.append((path.relative_to(root), name, fields))

    if not failures:
        print("wire casing: every tagged enum renames its fields explicitly")
        return 0

    print("Tagged enums shipping snake_case fields on a camelCase wire:\n")
    for path, name, fields in failures:
        print(f"  {name}  ({path})")
        for field in fields:
            print(f"      {field}")
    print(
        "\n`rename_all` on an enum renames the VARIANTS, not the fields inside\n"
        "them. Add a per-field attribute to each:\n"
        '\n    #[serde(rename = "camelCaseName")]\n'
        "\nUse the per-field form rather than the enum-level `rename_all_fields`:\n"
        "serde honours both, but utoipa's schema derive reads only the former, so\n"
        "the latter leaves the OpenAPI schema disagreeing with the JSON."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
