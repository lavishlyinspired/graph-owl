"""`gst:AmountMismatch` means nothing to a business user.

**The user's own words, and they are right.** Every Reco Now screen showed raw
rule IRIs. `gst:ITCNotAvailable` is a label a rule author chose; a reviewer
needs to know *what is wrong*, *why it matters*, and *what to do about it*.

**The information already existed and nothing asked for it.** `packs/gst`
carries a `[findings.guidance]` block per rule — `title`, `meaning`,
`next_action`, `tone` — and graph-owl serves it at `GET /packs/{pack}/console`.
Reco Now rendered the IRI instead.

**It stays in the pack, not in TypeScript.** A healthcare or banking pack names
different findings entirely, and guidance compiled into the console is guidance
that only ever fits one domain — the same reasoning that put the console's
field mapping in the pack.
"""

from __future__ import annotations

from app.rule_guidance import decorate, fallback_title


class TestFallbackWhenAPackSaysNothing:
    def test_an_iri_becomes_a_readable_phrase(self):
        """A rule with no guidance must still not show a colon-prefixed IRI.
        Splitting the camel case is not as good as an authored title, and it
        is much better than `gst:GoodsReceiptTiming`."""
        assert fallback_title("gst:GoodsReceiptTiming") == "Goods receipt timing"

    def test_an_acronym_survives_the_split(self):
        """`ITCNotAvailable` must not become `I T C Not Available`."""
        assert fallback_title("gst:ITCNotAvailable") == "ITC not available"

    def test_a_label_with_no_prefix_still_works(self):
        assert fallback_title("AmountMismatch") == "Amount mismatch"

    def test_a_label_with_digits_stays_readable_without_a_prefix(self):
        """Pinned as a property rather than an exact string. `Gstr1` and `In2b`
        are both letter-then-digit, so no consistent rule yields "Gstr1" *and*
        "in 2b" — and inventing a heuristic to make one example pretty would
        make another worse. Every real rule has an authored title; this only
        has to be readable for a rule nobody has written one for yet."""
        title = fallback_title("gst:Gstr1NotIn2b")

        assert ":" not in title
        assert title.startswith("Gstr1")
        assert "not" in title


class TestDecoratingWhatTheScreensShow:
    GUIDANCE = {
        "gst:AmountMismatch": {
            "title": "Values differ between your books and the portal",
            "meaning": "Both sides report this invoice and the amounts disagree.",
            "next_action": "Establish which side is right before filing.",
            "tone": "warning",
        }
    }

    def test_a_known_rule_gets_its_authored_title_meaning_and_action(self):
        rows = decorate([{"label": "gst:AmountMismatch"}], self.GUIDANCE)

        assert rows[0]["title"] == "Values differ between your books and the portal"
        assert rows[0]["meaning"]
        assert rows[0]["next_action"]

    def test_the_raw_label_is_kept_alongside_the_title(self):
        """A CA defending a position needs the rule's actual identifier, and
        so does anyone reading a log. Replacing it would trade one audience's
        problem for another's."""
        rows = decorate([{"label": "gst:AmountMismatch"}], self.GUIDANCE)

        assert rows[0]["label"] == "gst:AmountMismatch"

    def test_an_unknown_rule_falls_back_rather_than_rendering_an_iri(self):
        rows = decorate([{"label": "gst:SomethingBrandNew"}], self.GUIDANCE)

        assert rows[0]["title"] == "Something brand new"
        assert rows[0]["meaning"] is None

    def test_a_row_with_no_label_at_all_is_left_alone_not_crashed(self):
        rows = decorate([{"label": None}, {}], self.GUIDANCE)

        assert len(rows) == 2

    def test_decorating_does_not_mutate_the_row_it_was_given(self):
        """These rows come from the database layer and are reused. Mutating
        them in place makes the same object mean different things depending on
        which screen touched it first."""
        original = {"label": "gst:AmountMismatch"}

        decorate([original], self.GUIDANCE)

        assert "title" not in original

    def test_an_empty_guidance_map_still_produces_readable_titles(self):
        """A deployment whose pack has no `[console]` section, or a graph-owl
        that is unreachable, must still not show IRIs."""
        rows = decorate([{"label": "gst:PaymentOverdue"}], {})

        assert rows[0]["title"] == "Payment overdue"


class TestTheCallThatSilentlyDidNothing:
    """**Found 19 August 2026, and the bug class matters more than the bug.**

    `graphowl_client._request` was called by four features — the guidance
    fetch, the memory write, the waiver write and the explain read — and this
    module never defined or imported it. Every call site sat inside a bare
    `except Exception`, so the `NameError` was swallowed and each feature
    silently did nothing. It surfaced only because every rule rendered a
    *fallback* title, which looks like missing pack data rather than a broken
    call.

    The lesson is the bare except: `except Exception` around a call that can
    fail for **programming** reasons as well as network ones cannot tell the
    two apart, and reports both as "graph-owl was unavailable".
    """

    def test_the_helper_the_call_sites_depend_on_actually_exists(self):
        from app import graphowl_client

        assert callable(getattr(graphowl_client, "_request", None))

    def test_every_call_site_naming_it_can_resolve_it(self):
        """A grep-shaped test, deliberately. The failure was that a name used
        in four places existed in none, and only running each feature would
        have caught it — which is exactly what the bare excepts prevented."""
        import inspect

        from app import graphowl_client, main

        for module in (graphowl_client, main):
            source = inspect.getsource(module)
            if "graphowl_client._request(" in source or "\n    _request(" in source:
                assert hasattr(graphowl_client, "_request"), module.__name__

    def test_an_unreachable_server_yields_no_guidance_rather_than_raising(self):
        """The intended degradation, now that the accidental one is gone: no
        guidance costs authored titles, not the screen."""
        from app.graphowl_client import console_guidance

        assert console_guidance("http://127.0.0.1:1") == {}
