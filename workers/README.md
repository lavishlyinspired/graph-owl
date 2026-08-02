# Workers

Processes that do work graph-owl deliberately does not do in its own binary.

Today that is one thing: **document extraction** (Epic 21). PDF layout analysis,
OCR and LLM-based extraction have a Python ecosystem Rust does not come close to
matching, none of it is on graph-owl's read path, and all of it changes at the
pace of somebody else's library. `00j-language-boundaries.md` sets three tests
for moving something out of the binary; document parsing meets all three.

## What a worker is, and what it is not

A worker **proposes**. graph-owl **disposes**.

That is not a slogan — it is where the code lives. A worker sends a parsed
document and a list of claims, each with a confidence it chose for itself. On
receipt, and before anything is stored, graph-owl decides:

| Question | Decided by | Why not the worker |
|---|---|---|
| Is this confidence enough to assert? | `Disposition::for_confidence` | An extractor that graded its own certainty would be trusted exactly as much as it trusted itself |
| Is this predicate in the model? | `constrain` | Open information extraction produces a graph nothing can query |
| Is this subject a real entity? | the catalog | Only the catalog knows |
| Did a human already reject this? | the rejection index | The worker has no memory of other runs |

Every one of those checks runs on **every** claim from **every** source,
including graph-owl's own in-process markdown extractor, which gets no exemption
for being local. A worker cannot opt out of them, and that is the property that
makes running untrusted or merely mis-tuned extractors safe.

**A consequence worth stating plainly:** inflating a confidence buys a worker
nothing it could not have had honestly, and costs it the review that would have
caught it being wrong.

## Adding a worker

Nothing in graph-owl changes. There is no enum of extractor kinds to extend, no
migration to write, no Rust type to widen — a worker's identity travels as data
in each claim's provenance (`extractor`, `extractorVersion`). Adding one is a
deployment.

What a worker must do:

1. Produce a `ParsedDocument`: `sourceId`, `mediaType`, `text`, and optional
   `sections`. Text plus offsets, because that is the only shape a markdown
   file, a PDF, a scanned image and a chat export all have.
2. Produce claims whose `evidence` spans are **byte** offsets into that exact
   text. graph-owl resolves them against the string you sent, so a worker
   counting characters will point at the wrong words in any document containing
   an accent.
3. `POST /extraction/runs`.

`sdk/python/graph_owl_sdk/extraction.py` is that contract as dataclasses.

## python

`workers/python` — the reference worker. Handles markdown and plain text with no
optional dependency installed; PDF behind the `pdf` extra.

```bash
pip install -e sdk/python -e "workers/python[pdf]"
graph-owl-worker ./docs --server http://localhost:8080 --pdf
```

Exit codes: `0` everything handled, `1` some documents failed, `2` the run could
not start. A scheduler retrying a `2` is right; retrying a `1` usually is not,
because a corrupt PDF will still be corrupt in five minutes.

The extractor it ships with is deterministic and rule-based, and claims `0.6` —
inside graph-owl's *surface* band, so every claim waits for a human. A name
matched in prose is evidence, not proof. It exists to make the pipeline testable
for correctness rather than plausibility; an LLM extractor replaces that one
class and nothing else.
