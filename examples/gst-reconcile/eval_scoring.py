"""Precision, recall, and a Wilson interval over a sample — the scoring
math `packs/gst/eval/questions.md`'s own "Scoring" section specifies,
kept generic (no GST-specific data) so any future harness, human- or
agent-graded, can reuse it. Epic 105 P12
(`plans/105t-eval-scoring.md`).

Stdlib only, matching `reconcile_agent.py`'s own choice so
`scripts/check-examples-purity.py` passes and this runs against a bare
Python.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass

#: An invoice number as `questions.md`'s own subjects are named
#: (`INV-1003`), matched as a whole word so `INV-100` does not also match
#: inside `INV-1003`.
_INVOICE_MENTION = re.compile(r"\bINV-\d+\b")


@dataclass(frozen=True)
class FindingScore:
    """Precision and recall at the level of the finding — `questions.md`'s
    own definition: "a correct answer names the right invoices and no
    others."
    """

    precision: float
    recall: float

    @property
    def exact(self) -> bool:
        return self.precision == 1.0 and self.recall == 1.0


def score_finding(predicted: list[str], expected: list[str]) -> FindingScore:
    """Set-based precision/recall over two subject lists.

    Naming the same invoice twice in `predicted` is not double credit:
    `questions.md`'s own worked example ("names INV-1003 and INV-1004
    where the key says INV-1003 alone scores recall 1.0 and precision
    0.5") is a set comparison, not a sequence one, so both lists are read
    as sets here.

    The two empty-list edges are real cases, not malformed input: several
    questions expect no finding at all, and a prediction that also names
    nothing has scored correctly by reporting nothing (precision 1.0,
    recall 1.0). A non-empty prediction against an empty key has no true
    positives to divide by, so precision is 0 (everything named was
    wrong) and recall is vacuously 1 (there was nothing to miss); an empty
    prediction against a non-empty key is the mirror — precision
    vacuously 1 (nothing wrong was said), recall 0 (everything was
    missed). Either combination already fails `.exact`, so the vacuous
    half of each never reads as a correct answer on its own.
    """
    predicted_set, expected_set = set(predicted), set(expected)
    if not expected_set:
        return FindingScore(precision=1.0 if not predicted_set else 0.0, recall=1.0)
    if not predicted_set:
        return FindingScore(precision=1.0, recall=0.0)
    hits = predicted_set & expected_set
    return FindingScore(
        precision=len(hits) / len(predicted_set),
        recall=len(hits) / len(expected_set),
    )


def score_narration(text: str, expected: list[str]) -> FindingScore:
    """Score free-form prose against an expected invoice-number key.

    **The scoring convention questions 12, 13 and 15 needed and did not
    have.** Those three are answered by a free-form tool-calling agent
    (`gst_investigation_agent.py`, Epic 105 P11), not a fixed table — the
    investigator decides which tool to call next, so there is no
    structured `subjects` list to compare the way `score_finding` does
    for questions 1-11. This extracts every invoice number the answer
    text *mentions* and scores that set exactly the same way, so a
    narrated answer is held to the same "names the right invoices and no
    others" standard `questions.md` states once, rather than a separate,
    looser one for prose.

    A mention counts once even if the text repeats it (`score_finding`'s
    own set-based reading, unchanged here — narrating the reasoning
    naturally re-mentions an invoice without meaning extra credit).

    **Known limitation, accepted rather than solved**: this has no
    negation awareness. Prose that names an invoice specifically to rule
    it out ("...but not INV-2001") is scored as a false positive, the
    same as if it had claimed a finding for it. Question 8 is written
    exactly this way — a good narration mentions the invoice it is ruling
    out — so this systematically under-scores discrimination answers by
    one false positive rather than never. Sentence-level negation parsing
    would fix it and is a materially larger, separate undertaking; the
    honest cost is documented rather than hidden behind a looser match.
    """
    mentioned = _INVOICE_MENTION.findall(text)
    return score_finding(mentioned, expected)


def wilson_interval(successes: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """The Wilson score interval — accurate at the small `n` this
    evaluation set has (`questions.md`: "at fifteen questions the
    interval is wide, and pretending otherwise is how an evaluation stops
    being informative"), unlike the normal approximation it replaces.

    `z = 1.96` is the standard z-score for a 95% confidence interval, a
    public statistics constant rather than a number chosen for this
    system (`00i` rule 4).

    # Raises

    `ValueError` if `n` is zero — there is no interval over an empty
    sample, and returning one (however wide) would misreport "no data" as
    "maximum uncertainty about some data."
    """
    if n == 0:
        raise ValueError("cannot compute an interval over zero samples")
    phat = successes / n
    denominator = 1 + z**2 / n
    centre = phat + z**2 / (2 * n)
    spread = z * math.sqrt(phat * (1 - phat) / n + z**2 / (4 * n**2))
    return ((centre - spread) / denominator, (centre + spread) / denominator)
