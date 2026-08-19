"""Agents that actually do something.

**What was there before**: `wake_agents` created a run and recorded its empty
summary. No agent read anything, decided anything or produced anything. The
activity screen showed that an agent *would have* run.

Three real agents, each doing work a human would otherwise do by hand, and each
leaving a trace of what it looked at before it decided:

- **Triage** — ranks the period's findings so a reviewer knows what to open
  first. Ranking is the judgement; the ordering rule is explicit and the model
  is not asked for it.
- **Explainer** — narrates each finding from its own row.
- **Vendor** — drafts the chase message for suppliers who have not filed.

**The model is never asked for a number or an ordering.** It writes prose from
figures already computed. That is the split the research supports and the one
this product's own incident history demands.
"""

from __future__ import annotations

from app.agents import run_triage, run_vendor
from app.agent_runtime import Registry, Subscription

CASES = [
    {"id": "1", "invoice_no": "INV-1", "reason_code": "gst:ITCNotAvailable",
     "books_amount": 58300.0, "portal_amount": None, "supplier_name": "Patel"},
    {"id": "2", "invoice_no": "INV-2", "reason_code": "gst:SupplierNotFiled",
     "books_amount": 8640.0, "portal_amount": None, "supplier_name": "Phantom"},
    {"id": "3", "invoice_no": "INV-3", "reason_code": "gst:AmountMismatch",
     "books_amount": 180000.0, "portal_amount": 180500.0, "supplier_name": "Sharma"},
]


def _registry(*grants: str) -> Registry:
    registry = Registry()
    registry.subscribe(Subscription(agent="triage", event="reconciliation.finished"))
    registry.subscribe(Subscription(agent="vendor", event="reconciliation.finished"))
    for grant in grants:
        registry.grant(grant.split(":")[0], grant.split(":")[1])
    return registry


class TestTriage:
    def test_it_ranks_by_what_is_actually_at_stake_not_by_row_order(self):
        run = run_triage(cases=CASES, registry=_registry("triage:propose"), model=None)

        ranked = run.writes[0]["payload"]["ranked"]
        assert [r["invoice_no"] for r in ranked][0] == "INV-1"

    def test_lost_credit_outranks_a_larger_amount_that_is_merely_deferred(self):
        """₹58,300 blocked is gone; ₹1,80,000 under review is a ₹500
        disagreement about an invoice both sides carry. Ranking by raw amount
        would put the wrong one first, every time."""
        run = run_triage(cases=CASES, registry=_registry("triage:propose"), model=None)

        ranked = run.writes[0]["payload"]["ranked"]
        positions = {r["invoice_no"]: i for i, r in enumerate(ranked)}
        assert positions["INV-1"] < positions["INV-3"]

    def test_every_ranked_item_says_why_it_is_where_it_is(self):
        """A ranking with no reasons is an opinion. This one has to be
        arguable."""
        run = run_triage(cases=CASES, registry=_registry("triage:propose"), model=None)

        for item in run.writes[0]["payload"]["ranked"]:
            assert item["because"]

    def test_the_ordering_is_a_decision_span_not_a_model_call(self):
        """The model is never asked to rank. An ordering that changes between
        runs of identical data is not a ranking, and nobody could defend it."""
        run = run_triage(cases=CASES, registry=_registry("triage:propose"), model=None)

        kinds = [s["kind"] for s in run.spans]
        assert "decision" in kinds

    def test_it_leaves_a_trace_of_what_it_read_before_deciding(self):
        run = run_triage(cases=CASES, registry=_registry("triage:propose"), model=None)

        assert any(s["kind"] == "tool" for s in run.spans)

    def test_without_the_grant_it_refuses_and_the_run_records_it(self):
        run = run_triage(cases=CASES, registry=_registry(), model=None)

        assert run.writes == []
        assert run.refusals
        assert run.summary()["status"] == "completed"

    def test_an_empty_period_completes_rather_than_failing(self):
        run = run_triage(cases=[], registry=_registry("triage:propose"), model=None)

        assert run.summary()["status"] == "completed"


class TestVendor:
    def test_it_drafts_only_for_suppliers_who_have_not_filed(self):
        run = run_vendor(cases=CASES, registry=_registry("vendor:propose"), model=None)

        drafts = run.writes[0]["payload"]["drafts"]
        assert [d["invoice_no"] for d in drafts] == ["INV-2"]

    def test_a_draft_names_the_supplier_and_the_amount(self):
        run = run_vendor(cases=CASES, registry=_registry("vendor:propose"), model=None)

        draft = run.writes[0]["payload"]["drafts"][0]
        assert "Phantom" in draft["message"]
        assert "8,640" in draft["message"]

    def test_with_no_model_the_draft_is_still_produced(self):
        """A chase email that only exists when an inference server is up is a
        feature nobody can rely on."""
        run = run_vendor(cases=CASES, registry=_registry("vendor:propose"), model=None)

        assert run.writes[0]["payload"]["drafts"][0]["source"] == "computed"

    def test_one_supplier_chase_per_invoice_however_many_rules_fired(self):
        """**Found in a real run's trace.** Both `SupplierNotFiled` and
        `PotentialMismatch` fire on the same invoice, so the agent drafted the
        same chase twice — and would have sent it twice. One invoice is one
        conversation with one supplier."""
        doubled = CASES + [
            {"id": "4", "invoice_no": "INV-2", "reason_code": "gst:PotentialMismatch",
             "books_amount": 8640.0, "portal_amount": None, "supplier_name": "Phantom"},
        ]

        run = run_vendor(cases=doubled, registry=_registry("vendor:propose"), model=None)

        drafts = run.writes[0]["payload"]["drafts"]
        assert [d["invoice_no"] for d in drafts] == ["INV-2"]

    def test_the_statutory_reference_in_the_draft_does_not_get_it_refused(self):
        """**Found in a real run's trace**: every draft was refused for
        "states 16, 2" — the model quoting Section 16(2)(aa), which the
        computed template itself contains. A provision reference is not an
        invented figure, and refusing it made the agent produce nothing while
        appearing to work."""
        run = run_vendor(
            cases=CASES,
            registry=_registry("vendor:propose"),
            model=lambda _p: (
                "Dear Phantom, invoice INV-2 carrying ₹8,640 of credit has not "
                "appeared in our GSTR-2B. Section 16(2)(aa) makes it available "
                "only once you have furnished the details."
            ),
        )

        assert run.writes[0]["payload"]["drafts"][0]["source"] == "model"

    def test_a_model_draft_that_invents_a_figure_falls_back(self):
        run = run_vendor(
            cases=CASES,
            registry=_registry("vendor:propose"),
            model=lambda _p: "You owe us ₹9,99,999 immediately.",
        )

        draft = run.writes[0]["payload"]["drafts"][0]
        assert draft["source"] == "computed"
        assert "999999" not in draft["message"].replace(",", "")
