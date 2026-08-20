"""Routing free text typed into a search bar to one of the fixed
evaluation questions `reconcile_agent.answer()` can actually answer.

**A forced wrong match is worse than an honest "none of these."** These
tests exist specifically to pin that refusal — `best_match` returning
`None` for unrelated text is as load-bearing as it returning the right
key for related text.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ask_server import best_match, best_supplier_match, extract_supplier_query  # noqa: E402


def test_matches_the_real_wording_of_question_1() -> None:
    # `packs/gst/eval/questions.md`'s own phrasing for #1 — not the short
    # internal `.text` label, since that is not what a person types.
    assert best_match("Which July 2026 invoices did the supplier never file in GSTR-2B?") == 1


def test_matches_the_real_wording_of_question_5() -> None:
    assert best_match("Which invoices are unpaid past 180 days?") == 5


def test_matches_a_candidate_scoped_question_by_its_own_invoice_number() -> None:
    assert best_match("Is INV-1001 compliant?") == 6


def test_refuses_to_force_a_match_on_unrelated_text() -> None:
    assert best_match("What is the capital of France?") is None


def test_refuses_a_blank_query() -> None:
    assert best_match("") is None
    assert best_match("   ") is None


def test_picks_the_better_of_two_plausible_candidates() -> None:
    # Shares "invoices"/"reverse-charge" with Q4's own text
    # ("Matched, and flagged reverse-charge") more than with any other.
    assert best_match("Which invoices are flagged reverse-charge?") == 4


# ---- Supplier invoice count — the question none of the 15 fixed
# questions covers (found live: a real user asked "how many invoices are
# there for patel chemicals and co" and got an honest, unhelpful
# `noMatch`, even though `gst:supplierName`/`gst:issuedBy` make this a
# real, answerable graph query). ----


def test_extracts_the_supplier_name_after_for() -> None:
    assert extract_supplier_query("how many invoices are there for patel chemicals and co") == "patel chemicals and co"


def test_extracts_the_supplier_name_after_from() -> None:
    assert extract_supplier_query("which invoices are from Patel Chemicals & Co?") == "Patel Chemicals & Co"


def test_does_not_extract_from_a_question_naming_no_party() -> None:
    # Must not collide with question 5's own wording — no "for/from/by
    # <name>" clause here, just a common noun.
    assert extract_supplier_query("Which invoices are unpaid past 180 days?") is None


def test_does_not_extract_from_a_non_invoice_question() -> None:
    assert extract_supplier_query("what is the weather for tomorrow") is None


_SUPPLIERS = [
    ("1024:supplier-19AABCP8087C1ZV", "Patel Chemicals & Co"),
    ("1024:supplier-11AABCZ9999A1Z1", "Ghost Vendor Pvt Ltd"),
    ("1024:supplier-27AABCS1429B1Z8", "Sharma Infrastructure Pvt Ltd"),
]


def test_matches_a_supplier_by_partial_name() -> None:
    assert best_supplier_match("patel chemicals and co", _SUPPLIERS) == _SUPPLIERS[0]


def test_matches_a_supplier_despite_punctuation_differences() -> None:
    # Input says "and", the real name says "&" — must still match.
    assert best_supplier_match("patel chemicals & co", _SUPPLIERS) == _SUPPLIERS[0]


def test_refuses_to_force_a_supplier_match_on_an_unrelated_name() -> None:
    assert best_supplier_match("some company that does not exist", _SUPPLIERS) is None
