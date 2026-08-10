"""Reading a `pack.toml`, and the neutrality property the two real packs prove.

The manifest tests use `tmp_path` fixtures; the neutrality tests read the
**committed** `packs/hospitality` and `packs/gst` directly, because the claim
is about those files rather than about a shape a test invented.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from graph_owl_packs import Manifest, ManifestError

PACKS = Path(__file__).resolve().parents[3] / "packs"


def write(directory: Path, body: str) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "pack.toml").write_text(body, encoding="utf-8")
    return directory


MINIMAL = """
[pack]
id = "demo"
namespace = "https://example.org/ns/demo#"
prefix = "demo"
"""


# ── the manifest itself ──────────────────────────────────────────────────


def test_a_minimal_manifest_loads(tmp_path):
    manifest = Manifest.load(write(tmp_path / "demo", MINIMAL))

    assert manifest.id == "demo"
    assert manifest.namespace == "https://example.org/ns/demo#"
    assert manifest.prefix == "demo"
    assert manifest.documents == ()


def test_documents_are_read_in_order_with_their_source_and_format(tmp_path):
    directory = write(
        tmp_path / "demo",
        MINIMAL
        + """
[[documents]]
path = "ontology.ttl"
source = "demo-ontology"

[[documents]]
path = "fixtures/data.nt"
source = "demo-data"
format = "ntriples"
""",
    )

    manifest = Manifest.load(directory)

    assert [d.source for d in manifest.documents] == ["demo-ontology", "demo-data"]
    assert manifest.documents[0].format == "turtle", "turtle is the default"
    assert manifest.documents[1].format == "ntriples"


def test_a_missing_pack_toml_says_what_a_pack_is(tmp_path):
    with pytest.raises(ManifestError, match="pack.toml"):
        Manifest.load(tmp_path / "nothing-here")


def test_invalid_toml_names_the_file(tmp_path):
    directory = write(tmp_path / "demo", "[pack\nid = broken")

    with pytest.raises(ManifestError, match="not valid TOML"):
        Manifest.load(directory)


@pytest.mark.parametrize("missing", ["id", "namespace", "prefix"])
def test_every_required_key_is_required(tmp_path, missing):
    body = "\n".join(
        line for line in MINIMAL.strip().splitlines() if not line.startswith(f"{missing} ")
    )
    directory = write(tmp_path / "demo", body)

    with pytest.raises(ManifestError, match=missing):
        Manifest.load(directory)


def test_an_empty_required_value_is_refused_like_a_missing_one(tmp_path):
    # `id = ""` is a present key and an absent value. Accepting it would put an
    # empty pack id into a namespace declaration's provenance.
    directory = write(tmp_path / "demo", MINIMAL.replace('id = "demo"', 'id = "   "'))

    with pytest.raises(ManifestError, match="id"):
        Manifest.load(directory)


@pytest.mark.parametrize(
    "escape", ["../../../etc/passwd", "/etc/passwd", "fixtures/../../secret.ttl"]
)
def test_a_document_path_cannot_escape_the_pack_directory(tmp_path, escape):
    # **The loader uploads these bytes to a server that trusts it.** A pack
    # that could name any path on the filesystem would make installing one an
    # exfiltration primitive. Checked on the declaration, so it fails even
    # when the file does not exist — the file existing is not what makes the
    # declaration wrong.
    directory = write(
        tmp_path / "demo",
        MINIMAL + f'\n[[documents]]\npath = "{escape}"\nsource = "demo"\n',
    )

    with pytest.raises(ManifestError, match="inside the pack"):
        Manifest.load(directory)


def test_a_document_missing_its_source_is_refused(tmp_path):
    directory = write(
        tmp_path / "demo", MINIMAL + '\n[[documents]]\npath = "ontology.ttl"\n'
    )

    with pytest.raises(ManifestError, match="source"):
        Manifest.load(directory)


# ── the neutrality property, over the committed packs ────────────────────


def test_both_shipped_packs_load():
    for pack in ("hospitality", "gst"):
        manifest = Manifest.load(PACKS / pack)
        assert manifest.id == pack
        assert manifest.documents, f"{pack} lists no documents"


def test_every_document_a_shipped_pack_lists_actually_exists():
    # A manifest that names a file it does not have is a pack that fails
    # halfway through loading, after some of it has already landed.
    for pack in ("hospitality", "gst"):
        manifest = Manifest.load(PACKS / pack)
        for document in manifest.documents:
            assert (
                manifest.directory / document.path
            ).is_file(), f"{pack}: {document.path} is listed but missing"


def test_the_two_packs_share_no_vocabulary():
    # **This is the neutrality test.** Hospitality and GST were chosen to have
    # nothing in common — no statute, no identifier scheme, no subject matter.
    # If their namespaces or prefixes collided, the "same platform, different
    # data" claim would be resting on them being secretly similar.
    hospitality = Manifest.load(PACKS / "hospitality")
    gst = Manifest.load(PACKS / "gst")

    assert hospitality.namespace != gst.namespace
    assert hospitality.prefix != gst.prefix
    assert not set(d.source for d in hospitality.documents) & set(
        d.source for d in gst.documents
    ), "two packs sharing an import-graph source would overwrite each other"


def test_the_two_packs_configure_the_same_blocking_algorithms():
    # The other half of the same claim, and the more surprising one: these
    # domains share no *data*, and they share every *algorithm*. A strategy
    # named after a domain — `gstin_key`, `guest_phone_key` — would break
    # this, which is exactly what it is here to catch.
    hospitality = Manifest.load(PACKS / "hospitality")
    gst = Manifest.load(PACKS / "gst")

    def strategies(manifest) -> set[str]:
        found = set()

        def walk(entries):
            for entry in entries:
                found.add(entry["strategy"])
                walk(entry.get("of", []))

        walk(manifest.matching.get("blocking", []))
        return found

    hospitality_strategies = strategies(hospitality)
    gst_strategies = strategies(gst)

    assert "normalized" in hospitality_strategies & gst_strategies, (
        "the same algorithm serves a phone number and an invoice number"
    )
    # Every strategy either pack names must be one the engine implements —
    # a name the engine does not know is a pack asking for domain-specific
    # code, which is the thing packs may not do.
    known = {
        "exact",
        "normalized",
        "phonetic",
        "ngram",
        "numeric_bucket",
        "date_window",
        "composite",
    }
    unknown = (hospitality_strategies | gst_strategies) - known
    assert not unknown, f"strategies the engine does not implement: {unknown}"


def test_no_pack_ships_code():
    # `plans/105-domain-neutrality.md`: a pack contributes configuration to
    # surfaces that already exist. A `.tsx` or `.rs` inside a pack directory
    # would be a domain reaching into the console or the binary — the exact
    # failure the console section of the platform plan forbids.
    forbidden = {".rs", ".tsx", ".ts", ".jsx", ".py", ".css"}
    for pack in ("hospitality", "gst"):
        offenders = [
            path.relative_to(PACKS)
            for path in (PACKS / pack).rglob("*")
            if path.suffix in forbidden
        ]
        assert not offenders, f"{pack} ships code: {offenders}"
