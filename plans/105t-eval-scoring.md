# Plan: P12 — scoring the GST evaluation set

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: In progress, 11 August 2026, directly following P11
(`105s-langgraph-investigation-agent.md`).

**Files**: `examples/gst-reconcile/eval_scoring.py` (new),
`examples/gst-reconcile/test_eval_scoring.py` (new) — sibling to the
existing `reconcile_agent.py`/`test_reconcile_agent.py`, no other files
touched.

## Scope, cut down from what `questions.md` describes

`packs/gst/eval/questions.md`'s own "Scoring" section specifies precision
and recall at the finding level plus "a Wilson interval on a sample, not a
single number." It does not specify who or what produces the "predicted"
answer being scored — a deliberate reading, since the fifteen questions do
not have a uniform answer shape. Questions 1–5 and 14 are list-of-invoices
answers, for which finding-level precision/recall is exactly the right
metric. Questions 6–11 are mostly yes/no or "why" answers (is this
invoice compliant; why does 2002 fire and 2001 doesn't), and 12/13/15 mix
a citation, an explanation, or a per-invoice amount into the answer —
none of which reduce to a subject-list comparison without inventing a
scoring convention `questions.md` never states. Scoring those honestly is
future work, not silently approximated here.

**This slice scores what has a real, structured, machine-checkable
answer today**: `reconcile_agent.py`'s five deterministic answers (P9),
compared against the hand-derived key. It does not invent a normalization
scheme for the other ten.

## What was built

- `eval_scoring.py`:
  - `FindingScore(precision: float, recall: float)`, with an `.exact`
    property (`precision == recall == 1.0`) — the same "correct" bar
    `questions.md`'s own worked example uses ("a correct answer names the
    right invoices and no others").
  - `score_finding(predicted, expected) -> FindingScore` — set-based
    (questions.md's own worked example treats duplicate names as no
    extra credit, which only a set comparison gives), with explicit,
    stated conventions for the two empty-list edges: both empty is a
    correct "found nothing" (1.0/1.0); an empty prediction against a
    non-empty key is 0 recall or scores wrong overall.
  - `wilson_interval(successes, n, z=1.96) -> (low, high)` — the closed-form
    Wilson score interval. `z = 1.96` is documented as the standard 95%
    z-score (a public statistics constant, not a number chosen for this
    system — `00i` rule 4). Raises on `n == 0` rather than returning a
    misleadingly wide interval over no data.
- `test_eval_scoring.py`: unit tests for `score_finding`'s boundary cases
  (exact match, one false positive, one false negative, disjoint, both
  empty, duplicate-name no-extra-credit — the last taken directly from
  `questions.md`'s own worked example numbers) and three **hand-derived,
  not fetched or recalled** algebraic properties of the Wilson interval,
  checked before writing the implementation:
  - `successes == n` → upper bound is exactly `1.0`, for any `n`/`z`
    (spread and centre both reduce to `z²/(2n)` above `phat=1`, so
    `centre + spread = 1 + z²/n`, exactly the denominator).
  - `successes == 0` → lower bound is exactly `0.0` (the mirror case).
  - `successes == n/2` → the interval's midpoint is exactly `0.5`
    (`centre/denominator` simplifies algebraically to `0.5` independent
    of `z` and `n`).
  - A comparative property: the same proportion at 10x the sample size
    gives a narrower interval.

  A first attempt at this reached for a worked numeric example from a
  reference source instead of deriving one — caught by this project's own
  "verify external formats via real web research" lesson before it went
  further: a fetched Wikipedia page's formula rendered as garbled math
  markup and had no worked example at all, and a second source returned
  403. Hand-deriving three exact boundary identities sidesteps needing an
  external decimal example altogether, and is more rigorous than trusting
  a half-remembered one would have been.

## The integration proof

`test_eval_scoring.py` also runs `score_finding` against
`test_reconcile_agent.py`'s own real `FINDINGS` fixture ("the nine
findings the real reconciliation produces... taken from a real run, not
invented") for all five covered questions, asserting every one scores
`.exact` — the scoring machinery applied to output a real system actually
produced, not only to synthetic cases — then computes
`wilson_interval(5, 5)` over that sample and asserts the lower bound is
positive (a 5-for-5 sample is still informative, just with a wide floor —
consistent with `questions.md`'s own warning that a small sample's
interval is wide, not that it says nothing).

## What this deliberately does not do

- **Does not score questions 6–15 automatically.** Ten of fifteen
  questions have no invoice-list answer shape; inventing one to force a
  uniform metric would score something `questions.md` never asked for.
- **Does not run P11's agent against a live model.** No `LLM_API_BASE_URL`
  is configured in this environment (checked, same as `105s`); P11's own
  RED test proves the *wiring* with a scripted model, which is not the
  same claim as a scored answer from a real model, and this slice does
  not conflate the two.
- **No mutmut report**, for the identical reason `105s` already
  documented and re-derived here rather than re-discovered: mutmut's
  module-key resolution does not fit a script imported via
  `sys.path.insert` outside an installable package, which is the same
  convention this file follows to stay consistent with
  `reconcile_agent.py`'s own test.
