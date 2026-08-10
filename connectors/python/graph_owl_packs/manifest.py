"""Reading a `pack.toml` — the domain-pack manifest.

**TOML, not YAML, and the reason is a dependency rather than a preference.**
Python's standard library has parsed TOML since 3.11 (`tomllib`) and has never
parsed YAML. A YAML manifest would put PyYAML in the dependency tree of
everything that loads a pack — including the reference applications, whose
whole value depends on importing nothing but the standard library and
`graph_owl_sdk` (Epic 36 Slice A, enforced by `check-examples-purity.py`).
A manifest is configuration rather than deeply-nested data, which is exactly
the shape TOML is good at.

**Everything a domain needs is data here.** The loader has no knowledge of
hospitality, tax or automotive; it reads a namespace, a list of documents and
some configuration, and every one of those is a string the pack supplies.
`plans/105-domain-neutrality.md` is the standing record of that claim, and
`packs/hospitality/` is the test of it.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path


class ManifestError(ValueError):
    """A `pack.toml` that cannot be trusted to describe a pack.

    Every message names the file and the key, because the person reading it is
    holding a text editor, not a debugger.
    """


@dataclass(frozen=True)
class Document:
    """One RDF document the pack lands, and where it lands."""

    path: str
    """Relative to the pack directory. Never absolute, never `..` — see
    [`Manifest.load`]."""
    source: str
    """Names the import graph (`graph:import:{source}`) and the unit a later
    delete removes. The server validates it again; validating here too means a
    typo is caught before any HTTP call rather than halfway through a load."""
    format: str = "turtle"


@dataclass(frozen=True)
class Predicate:
    """One predicate the pack asserts, and the shape its objects must take.

    **Declared, never inferred.** The import path refuses a flake whose
    predicate is unknown, and auto-registering whatever a document mentions
    would make a typo permanent — the graph would accept `gst:invoiceNumbre`
    forever, silently, beside the real one.
    """

    name: str
    """Local name, without the prefix. The namespace is the pack's own."""
    value_type: int = 1
    """`graph_owl_core::flake::value_type`. 1 is String — what a pack's
    descriptive predicates overwhelmingly are. 0 is Ref, for a predicate whose
    object is another subject."""
    many: bool = False


@dataclass(frozen=True)
class Manifest:
    """A pack, as its `pack.toml` describes it."""

    id: str
    namespace: str
    prefix: str
    directory: Path
    description: str = ""
    documents: tuple[Document, ...] = ()
    matching: dict = field(default_factory=dict)
    findings: tuple[dict, ...] = ()
    console: dict = field(default_factory=dict)
    predicates: tuple[Predicate, ...] = ()

    @staticmethod
    def load(directory: Path) -> "Manifest":
        """Read `<directory>/pack.toml`.

        # Raises

        `ManifestError` when the file is missing, unparsable, or missing a key
        the loader cannot proceed without.
        """
        path = directory / "pack.toml"
        if not path.is_file():
            raise ManifestError(f"{path} does not exist — a pack is a directory with a pack.toml")

        try:
            raw = tomllib.loads(path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as bad:
            raise ManifestError(f"{path} is not valid TOML: {bad}") from bad

        pack = raw.get("pack")
        if not isinstance(pack, dict):
            raise ManifestError(f"{path} has no `[pack]` table")

        for required in ("id", "namespace", "prefix"):
            value = pack.get(required)
            if not isinstance(value, str) or not value.strip():
                raise ManifestError(
                    f"{path}: `pack.{required}` is required and must be a non-empty string"
                )

        documents = []
        for index, entry in enumerate(raw.get("documents", [])):
            if not isinstance(entry, dict):
                raise ManifestError(f"{path}: `[[documents]]` entry {index} is not a table")
            for required in ("path", "source"):
                if not isinstance(entry.get(required), str) or not entry[required].strip():
                    raise ManifestError(
                        f"{path}: `documents[{index}].{required}` is required"
                    )
            # **A pack may not read outside its own directory.** A `path` of
            # `../../etc/passwd` would otherwise be uploaded verbatim to a
            # server that trusts the loader. Checked here rather than at the
            # filesystem, because the check must fail on the *declaration*
            # even when the file happens not to exist.
            candidate = Path(entry["path"])
            if candidate.is_absolute() or ".." in candidate.parts:
                raise ManifestError(
                    f"{path}: `documents[{index}].path` must stay inside the pack "
                    f"directory, got `{entry['path']}`"
                )
            documents.append(
                Document(
                    path=entry["path"],
                    source=entry["source"],
                    format=entry.get("format", "turtle"),
                )
            )

        predicates = []
        for index, entry in enumerate(raw.get("predicates", [])):
            if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
                raise ManifestError(f"{path}: `predicates[{index}].name` is required")
            predicates.append(
                Predicate(
                    name=entry["name"],
                    value_type=int(entry.get("value_type", 1)),
                    many=bool(entry.get("many", False)),
                )
            )

        return Manifest(
            id=pack["id"],
            namespace=pack["namespace"],
            prefix=pack["prefix"],
            directory=directory,
            description=pack.get("description", ""),
            documents=tuple(documents),
            matching=raw.get("matching", {}),
            findings=tuple(raw.get("findings", [])),
            console=raw.get("console", {}),
            predicates=tuple(predicates),
        )
