"""Scoring `eval_scoring.py`'s own math — Epic 105 P12
(`plans/105t-eval-scoring.md`).

The Wilson-interval assertions are three properties **hand-derived by
algebra before this file's implementation was written**, not pulled from
a fetched or remembered worked example — a first attempt reached for a
reference numeric example instead and found the fetched source's formula
rendered as garbled math markup with no worked example at all. Deriving
exact boundary identities (`successes == n`, `successes == 0`,
`successes == n / 2`) sidesteps needing an external decimal example
altogether.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from eval_scoring import FindingScore, score_finding, score_narration, wilson_interval  # noqa: E402
from reconcile_agent import answer  # noqa: E402
from test_reconcile_agent import FINDINGS  # noqa: E402


def test_an_exact_match_scores_one_and_one():
    assert score_finding(["a", "b"], ["a", "b"]) == FindingScore(precision=1.0, recall=1.0)


def test_a_false_positive_costs_precision_not_recall():
    """`questions.md`'s own worked example: naming INV-1003 and INV-1004
    where the key says INV-1003 alone scores recall 1.0, precision 0.5."""
    score = score_finding(["INV-1003", "INV-1004"], ["INV-1003"])
    assert score == FindingScore(precision=0.5, recall=1.0)
    assert not score.exact


def test_a_false_negative_costs_recall_not_precision():
    score = score_finding(["a"], ["a", "b"])
    assert score == FindingScore(precision=1.0, recall=0.5)


def test_disjoint_answers_score_zero_on_both():
    assert score_finding(["a"], ["b"]) == FindingScore(precision=0.0, recall=0.0)


def test_naming_a_subject_twice_gives_no_extra_credit():
    """Both lists are read as sets — duplicate names do not inflate a
    score, matching `questions.md`'s own definition of correctness."""
    assert score_finding(["a", "a"], ["a"]) == FindingScore(precision=1.0, recall=1.0)


def test_correctly_reporting_nothing_is_an_exact_match():
    """Several questions (6: the one wholly clean invoice; 9's underlying
    'no finding of this kind') expect no finding at all — a predicted
    empty answer against an empty key must score as correct, not as an
    edge case that happens to look like failure."""
    score = score_finding([], [])
    assert score.exact


def test_predicting_something_when_nothing_is_expected_is_wrong():
    score = score_finding(["a"], [])
    assert not score.exact
    assert score.precision == 0.0


def test_predicting_nothing_when_something_is_expected_is_wrong():
    score = score_finding([], ["a"])
    assert not score.exact
    assert score.recall == 0.0


def test_wilson_interval_rejects_an_empty_sample():
    with pytest.raises(ValueError, match="zero samples"):
        wilson_interval(0, 0)


def test_wilson_upper_bound_is_exactly_one_when_every_trial_succeeded():
    """Hand-derived: at `phat = 1`, `spread` reduces to `z**2 / (2n)`,
    the same term `centre` adds — so `centre + spread == 1 + z**2 / n`,
    which is exactly the interval's own denominator. True for any `n`
    and `z`, not just the case checked here."""
    for n in (1, 5, 50):
        _, upper = wilson_interval(n, n)
        assert upper == pytest.approx(1.0)


def test_wilson_lower_bound_is_exactly_zero_when_every_trial_failed():
    """The mirror case: at `phat = 0`, `centre - spread == 0` exactly."""
    for n in (1, 5, 50):
        lower, _ = wilson_interval(0, n)
        assert lower == pytest.approx(0.0, abs=1e-9)


def test_wilson_interval_is_centred_on_one_half_at_the_midpoint():
    """Hand-derived: at `phat = 0.5`, `centre / denominator` simplifies
    algebraically to exactly `0.5`, independent of `z` and `n`."""
    for n in (2, 10, 100):
        lower, upper = wilson_interval(n // 2, n)
        assert (lower + upper) / 2 == pytest.approx(0.5)


def test_more_samples_at_the_same_proportion_narrows_the_interval():
    narrow_lo, narrow_hi = wilson_interval(80, 100)
    wide_lo, wide_hi = wilson_interval(8, 10)
    assert (narrow_hi - narrow_lo) < (wide_hi - wide_lo)


def test_score_narration_extracts_a_single_mention():
    text = "The invoice at risk is INV-2001, filed under Notification 75/2019-CT."
    assert score_narration(text, ["INV-2001"]).exact


def test_score_narration_does_not_match_a_number_that_merely_starts_with_the_key():
    """`INV-100` must not score as a match for `INV-1003` — a prefix
    coincidence is not the same invoice, and a looser match would inflate
    recall on a wrong answer."""
    score = score_narration("See INV-100 for details.", ["INV-1003"])
    assert score.precision == 0.0


def test_score_narration_finds_every_distinct_invoice_named():
    text = "Both INV-1002 and INV-2002 disagree with GSTR-2B."
    score = score_narration(text, ["INV-1002", "INV-2002"])
    assert score.exact


def test_score_narration_cannot_tell_a_ruled_out_invoice_from_a_named_one():
    """A known, deliberate limitation, stated as a test rather than found
    later by a wrong-looking score: mention extraction has no negation
    awareness, so prose that names an invoice specifically to rule it out
    ("...but not INV-2001") is scored as though it claimed a finding for
    it. `questions.md`'s own question 8 is written exactly this way — the
    discrimination is the point of the question, and a narration answering
    it well is expected to mention the invoice it is ruling out. This
    documents the honest cost of a text-mention proxy rather than
    building sentence-level negation parsing to chase an exact score."""
    text = "INV-2002 is a finding under the 10% cap; INV-2001 is not, since its 5% delta is within it."
    score = score_narration(text, ["INV-2002"])

    assert not score.exact
    assert score.recall == 1.0
    assert score.precision < 1.0


def test_score_narration_repeating_a_mention_gives_no_extra_credit():
    text = "INV-1005 has no credit. To repeat: INV-1005 has no usable credit."
    assert score_narration(text, ["INV-1005"]).exact


def test_score_narration_of_prose_naming_no_invoice_matches_an_empty_key():
    assert score_narration("Nothing here is a compliance finding.", []).exact


def test_every_deterministic_answer_reconcile_agent_produces_scores_exact():
    """The scoring machinery applied to a real system's real output, not
    only to synthetic cases — `FINDINGS` is `test_reconcile_agent.py`'s
    own fixture, taken from a real run.

    Questions 6, 7 and 10 expect an **empty** key — `questions.md`'s own
    "No" answers — which `score_finding`'s empty/empty case
    (`test_correctly_reporting_nothing_is_an_exact_match`, above) already
    established scores `.exact`, not as a special case here but as the
    same machinery applied to a real negative.
    """
    key = {
        1: ["pr-INV-1003", "pr-INV-1004"],
        2: ["pr-INV-1002", "pr-INV-2002"],
        3: ["pr-INV-1005"],
        4: ["pr-INV-1006"],
        5: ["purchase-INV-1003", "purchase-INV-2002"],
        6: [],
        7: [],
        8: ["pr-INV-2002"],
        9: ["pr-INV-1005"],
        10: [],
        11: ["pr-INV-1004"],
    }
    scores = [
        score_finding(answer(number, FINDINGS).subjects, expected)
        for number, expected in key.items()
    ]
    assert all(score.exact for score in scores), scores

    lower, _ = wilson_interval(len(scores), len(scores))
    assert lower > 0.0, "eleven of eleven correct is still an informative lower bound"
