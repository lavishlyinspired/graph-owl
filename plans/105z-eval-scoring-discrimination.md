# Plan: P12 — scoring the discrimination and multi-hop questions

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, directly following `105t-eval-scoring.md`.
**Files**: `examples/gst-reconcile/reconcile_agent.py`,
`examples/gst-reconcile/test_reconcile_agent.py`,
`examples/gst-reconcile/eval_scoring.py`,
`examples/gst-reconcile/test_eval_scoring.py`,
`integrations/langchain/examples/gst_investigation_agent.py`,
`integrations/langchain/tests/test_gst_investigation_agent.py`.

## The gap, and the correction to `105t`'s own assessment

`105t`'s "What this deliberately does not do" said: "Does not score
questions 6-15 automatically. Ten of fifteen questions have no
invoice-list answer shape." That was too pessimistic — checked properly
against `packs/gst/eval/questions.md`'s own answer key, six of those ten
(6, 7, 8, 9, 10, 11) score exactly the same way 1-5 already do: a set of
invoice subjects compared by `score_finding`. Only 12, 13 and 15 (plus the
already-scored 14) genuinely need a free-form investigator rather than a
fixed table — and of those, only 12 has no invoice-shaped answer at all.

## What was built

**`reconcile_agent.py`'s `QUESTIONS` table extended, 5 → 11 entries.**
Generalized `QuestionSpec` from `(label, text)` to `(text, labels,
candidates)`:

- `candidates=None` (unchanged shape, questions 1-5, 11): every invoice
  carrying one of `labels`.
- `candidates=(...)` (new, questions 6-10): narrows to specific invoices
  — "is INV-1001 compliant" (6) needs every label checked against one
  invoice, not one label checked against every invoice.

A new `_invoice_number()` helper extracts the trailing `INV-\d+` a
subject's local name carries, because the same invoice is prefixed
differently by different finding kinds (`pr-INV-1003` vs
`purchase-INV-1003`) — matching on the suffix, not the whole name, and
raising rather than silently excluding a subject it cannot place.

**Nine of the eleven questions turned out to be existing-shape
extensions**, not new capability: 6, 7, 10 are absence checks (expect an
**empty** answer — `score_finding`'s empty/empty case, already
established in `105t`, now exercised on a real negative rather than only
a synthetic one); 8 is a two-candidate discrimination (does the rule fire
for INV-2002 but not INV-2001, over the identical `AmountMismatch`
label); 9 is question 3 reached by asking about one named invoice instead
of the class — asserted to produce the *same* answer as 3, so the two
question texts cannot quietly diverge; 11 is a plain label lookup
(`GstinTransposition`) the table never had a row for.

**Questions 12, 13 and 15 stay off the table, deliberately** — not an
oversight, a documented boundary in `QUESTIONS`'s own doc comment: 12
needs a traversal decision (which provision was in force) no fixed label
captures; 13 needs the exact-match failure and the similarity candidate
joined, which is what the free-form investigator (`gst_investigation_agent.py`,
Epic 105 P11) exists for; 15's own key cites INV-1004 under the matching
policy alone, *not* also under `PotentialMismatch` — the transposition
finding supersedes the potential-mismatch one for that invoice, and
widening the table to guess that would score confidently wrong rather
than honestly absent.

**`eval_scoring.py` gained `score_narration(text, expected)`** — the
scoring convention 12/13/15 were missing entirely. Extracts every
`INV-\d+` mention from free prose and scores the set exactly like
`score_finding`. **Documented, not hidden, limitation**: no negation
awareness — prose that names an invoice specifically to rule it out
scores as a false positive, the same as if it had claimed a finding for
it. Question 8's own key is written that way (mentioning the invoice it
rules out is what a good answer does), so this systematically costs one
false positive on that shape of question rather than never. Sentence-level
negation parsing would fix it and is a materially larger, separate
undertaking than the mention-extraction this needed today.

**`gst_investigation_agent.py` gained `score_investigation()` and
`--score-all`**, closing the "build the path, run it when you provide a
key" half of this work: `SCORED_QUESTIONS` maps 13 and 15 (not 12 — its
key names no invoice, so `score_narration` cannot honestly score it) to
their exact `questions.md` text and expected invoice set; `score_investigation`
runs a real investigation and scores the narration; `--score-all` runs
both and reports a Wilson interval, the same shape `105t`'s own
`test_eval_scoring.py` established for the deterministic questions.
Reaches `eval_scoring.py` via the same `sys.path.insert` pattern this
repository's own test files already use to cross the
`examples/gst-reconcile` (stdlib-only) / `integrations/langchain`
(LangChain-dependent) boundary without either package depending on the
other's tooling.

## Mutation testing

**No mutmut report**, for the identical, already-documented reason `105s`
and `105t` both give: mutmut's module-key resolution does not fit a
script imported via `sys.path.insert` outside an installable package.
Coverage instead comes from the mutation-aware scan this project's own
`testing` skill asks for before writing a RED test, applied to every
boundary the extension introduced — each has its own assertion:

- `candidates is None` vs a real tuple (both branches exercised: 1-5/11
  vs 6-10).
- The `_invoice_number` regex's word-boundary anchoring (`test_score_narration_
  does_not_match_a_number_that_merely_starts_with_the_key`) — `INV-100`
  must not match `INV-1003`.
- The two-candidate discrimination (question 8) asserts exactly one of
  the two candidates survives, not both and not neither.
- Question 9 asserted equal to question 3's own answer, so the two rows
  cannot silently diverge without a test noticing.
- `score_narration`'s negation blind spot pinned as its own test
  (`test_score_narration_cannot_tell_a_ruled_out_invoice_from_a_named_one`)
  rather than left as an undocumented surprise the first real narration
  would hit.

All four touched test files pass in full: `test_reconcile_agent.py` (27
tests, was 20), `test_eval_scoring.py` (20 tests, was 14),
`test_gst_investigation_agent.py` (6 tests, was 2),
`test_langgraph_integration.py` (unaffected, still passing) — 81 tests
total across the `integrations/langchain` suite, 47 across
`examples/gst-reconcile`.

## What this deliberately does not do

- **Does not score question 12.** Its answer key is a rate and a
  notification number, naming no invoice — `score_narration`'s
  invoice-mention convention has nothing to compare an empty expected set
  against except "did the text also name no invoice," which would score a
  wrong answer as correct by coincidence. A citation-matching convention
  is real, separate work.
- **Does not run either agent against a live model.** No
  `LLM_API_BASE_URL` is configured in this environment — the same
  "checked, same as 105s" posture `105t` already took, restated here
  rather than re-verified, since nothing about the environment changed.
  `--score-all` and `score_investigation` are proven against a scripted
  model (`test_score_investigation_scores_a_correct_narration_as_exact`
  and its siblings), which proves the wiring, not a real model's
  reasoning — the same distinction `105s`'s own RED test already drew.
- **Does not fix `score_narration`'s negation blindness.** Documented as
  a known, accepted cost above, not silently absorbed into a wider
  test-passing definition of "exact."
