# Incorporate OvisOCR2 as a document parser — Epic 21 follow-up

> Status: **Shipped, 10 August 2026.** All four slices built. Promoted into
> `plans/` from a design document that lived under the gitignored
> `.claude/docs/referencePlans/` — the same structural gap Epic 14 Slice H
> hit (a plan invisible to `scripts/epic-status.py` because it never reached
> a committed location).
> Type: **implementation plan** — a new optional parser adapter for the
> `graph-owl-worker` Python package, extending Epic 21
> (`plans/21-document-ingestion.md`).
> Scope: **zero Rust changes.** OvisOCR2 is an out-of-process Python worker,
> per `plans/00j-language-boundaries.md` and Epic 21 decision 0. Extractor
> identity travels as data (`extractor`, `extractorVersion`); the wire shape
> is bytes → `ParsedDocument` → `POST /extraction/runs` → `graph:extraction`.
> Nothing in a Rust crate or a migration changed.
> Decisions adopted: **served-endpoint client** (worker talks
> OpenAI-compatible HTTP to a separately-served model, never loads the
> weights), **scanned-PDF rasterization in scope**, **OCR always wins for
> `application/pdf`** when both `--ocr` and `--pdf` are set.

## Objective

Make graph-owl able to parse **scanned documents and page images** into the
same `ParsedDocument` pipeline that markdown and text-layer PDFs already flow
through, so their claims land in `graph:extraction` with provenance,
confidence bands, and idempotence — using **OvisOCR2** as the model.

OvisOCR2 (Alibaba ATH-MaaS, released 2026-07-15, **Apache-2.0**) is a 0.8B
end-to-end page-level document parser: page image → Markdown in natural
reading order, covering text, tables (HTML), formulas (LaTeX), and figure
regions (bbox tags). It is the first end-to-end model to top **OmniDocBench
v1.6 (96.58)** and leads **PureDocBench (75.06)**, ahead of pipeline stacks
(PaddleOCR-VL-1.6, MinerU2.5-Pro, GLM-OCR). Weights ~0.9B; FP16 inference ≈
1.9 GB VRAM, INT4 ≈ 0.5 GB — fits a single consumer GPU.

Epic 21 deferred vision-model parsing explicitly: *"Layout-model-based
parsing (vision models) → the port allows it; no adapter planned until
accuracy on real documents justifies the dependency"*
(`21-document-ingestion.md`). OvisOCR2 topping OmniDocBench is that
justification. This plan crosses the deferral off.

## Why a served endpoint, not in-process vLLM

The model card's default is an in-process vLLM load (`vllm==0.22.1`, NVIDIA
CUDA). That is rejected here for three reasons, all consistent with how this
project already draws boundaries:

1. **`00j-language-boundaries.md`'s flip rule**: when a capability needs
   Python-only heavy lifting (an embedding model, a document parser), "the
   heavy lifting moves" — out of process, behind a service. A worker that
   loads a model is a different deployment shape than a worker that calls
   one.
2. **The worker stays a batch job on a scheduler's schedule.** Loading and
   holding 0.8B of weights in a process whose other job is rule-based
   extraction gives every run a GPU-sized startup cost and a GPU-sized
   failure surface.
3. **Determinism and testability.** A client of an OpenAI-compatible
   endpoint is trivially fake-testable with a scripted double — a GPU never
   needs to exist in CI. In-process vLLM is a real compile/runtime
   dependency that would live in the `graph-owl-worker` extra and force CI
   to either install it or test only the assembly.

The endpoint (vLLM `serve`, SGLang, or a GGUF/llama.cpp server) is **the
deployment a user opts into**, exactly like the Postgres-only-vs-connector
split: a deployment that only ingests markdown still runs no model service.

Where this decision flips: if a future deployment cannot run a separate
model service (offline air-gapped single host), an `InProcessOcrModel`
behind the same `OcrModel` seam is a day's work — the seam is designed so
the choice of runtime never touches the parser assembly.

## The seam (`OcrModel`)

```
image/png | image/jpeg | image/webp ─┐
                                      ├─→ OvisOcrParser ──┐
PDF ── pypdfium2 ──→ pages ─────────→ OcrPdfParser ───────┤
                                                          ├─→ OcrModel (seam)
                                                          │     ├─ EndpointOcrModel ──→ POST /v1/chat/completions (served vLLM/SGLang/llama.cpp)
                                                          │     └─ (future) InProcessOcrModel
                                                          ├─→ markdown → byte-offset sections → ParsedDocument
                                                          └─→ sha256 fingerprint → POST /extraction/runs → graph:extraction
```

**`OcrModel` is the testability boundary.** A single callable-shaped
interface:

```python
class OcrModel(Protocol):
    def parse_images(self, images: list[Image.Image]) -> list[str]:
        """One Markdown string per page image, in input order."""
        ...
```

- The parser assembly (`OvisOcrParser`, `OcrPdfParser`) depends only on this
  interface. Unit tests inject a scripted fake returning fixed Markdown; **no
  GPU, no model, no network in any test that is not explicitly the
  endpoint-client's own.**
- The one shipped implementation is `EndpointOcrModel`: stdlib
  `urllib.request`, one `POST /v1/chat/completions` per page (per-page error
  isolation), message content `[{"type": "image_url", "image_url": {"url":
  "data:image/png;base64,..."}}, {"type": "text", "text": PROMPT}]`,
  `temperature=0.0`, `max_tokens=16384`.
- The endpoint is configurable: `--model` (served model name, default
  `ATH-MaaS/OvisOCR2`), `--ocr-endpoint` (default `http://localhost:8000`),
  `--ocr-dpi` (rasterization, default `200`), `--prompt-file`.

## Where it slots into the existing worker

`workers/python/graph_owl_worker/` is the reference worker
(`workers/README.md`). The pattern mirrors `PdfParser` + `--pdf` + the `pdf`
extra:

| Existing (Epic 21) | New (this plan) |
|---|---|
| `parsers.py::PdfParser` — deferred `pypdf` import, `handles() -> ("pdf", "application/pdf")`, typed `ParseError` | `ocr.py::OvisOcrParser` (`image/png`, `image/jpeg`, `image/webp`) + `OcrPdfParser` (`application/pdf`) |
| `cli.py --pdf` flag — construct only when asked, `UnsupportedMediaType` → stderr + `EXIT_UNUSABLE` | `cli.py --ocr` flag, same discipline, plus `--ocr-endpoint`/`--model`/`--prompt-file`/`--ocr-dpi` |
| `pyproject.toml` `pdf = ["pypdf>=4"]` | `ovis-ocr2 = ["Pillow>=10", "pypdfium2>=4"]` (image decode + rasterizer only; the model is a network endpoint) |
| `worker.py::MEDIA_TYPES` — `.md .markdown .txt .pdf` | added `.png .jpg .jpeg .webp` → `image/png`, `image/jpeg`, `image/webp` |

**Routing rule (decision: OCR always wins for PDF).** When `--ocr` is set,
`application/pdf` routes to `OcrPdfParser` (rasterize → OCR), and the
text-layer `PdfParser` is used only when `--ocr` is off. Implemented by
registration order in `ParserRegistry` (registered parsers are prepended, so
`cli.py` registers `PdfParser` first when `--pdf` is set, then `OcrPdfParser`
when `--ocr` is set — the OCR parser ends up first and wins). One code path
to test; predictable. The trade-off accepted: a digital PDF OCRs at endpoint
cost rather than free text extraction — the operator who sets `--ocr` asked
for that. Verified directly: `test_ocr_wins_application_pdf_over_pdf_when_both_flags_are_set`
in `workers/python/tests/test_cli.py`.

**Bytes → sections.** `ParsedDocument` = `{sourceId, mediaType, text,
sections?}`; spans are **byte** offsets into `text` (the cross-language
contract both sides assert — a Python worker counting characters would
silently mispoint spans in any non-ASCII document). Sections are
**page-per-section** (`"page N"`), the same coordinate `PdfParser` uses — the
coordinate a human checking a claim against the original will actually use.
OvisOCR2's Markdown headings are not trusted as section boundaries because an
OCR model's heading inference is exactly the unreliable layer this pipeline
exists to not depend on.

**Text hygiene.** Two model-output post-processors, both pure and
unit-tested (Slice 1):
- **`filter_imgtags`**: drop `<img src="images/bbox_{left}_{top}_{right}_{bottom}.jpg" />`
  blocks (the model's figure-placeholder convention, bbox scale `[0, 1000)`).
  Figures carry no extractable claims; leaving the tags in would put HTML
  noise in evidence text and in the fingerprint.
- **Truncated-repeat cleaner**: a vision model pushing a long page toward its
  output ceiling can repeat the final block instead of stopping. The cleaner
  trims a detected repeating suffix, only when it is big enough to be a
  failure rather than a coincidence.

## Magic numbers — each with a stated reason (00i rule 4)

| Number | Value | Reason |
|---|---|---|
| `temperature` | `0.0` | Determinism is a stated acceptance criterion of Epic 21 Slice B ("extraction is deterministic for a fixed input and model version"). Greedy sampling. |
| `max_tokens` | `16384` | A table/LaTeX-heavy page routinely exceeds an 8K budget, and truncation mid-table is the failure the repeat-cleaner exists to contain. |
| rasterization `--ocr-dpi` | `200` | The commonly-cited floor for legible OCR text recognition — below it, small body text degrades into ambiguous glyphs; well above it, page images grow faster than recognition accuracy improves. Configurable. |
| `filter_imgtags` | on | Figures carry no extractable claims; see "Text hygiene". |
| repeat-cleaner constants | `min_text_len=8000, max_period=200, min_period=1, min_repeat_chars=100, min_repeat_times=5` | Model-output hygiene for truncated generations; configurable, and only ever applied to model output, never to a hash input before the fingerprint is pinned. |
| bbox scale | `[0, 1000)` | The figure-placeholder convention `OCR_PROMPT` itself specifies (originally authored for this project — see Licensing below); consumed, never produced, only to filter tags. |
| pixel budgets | — | **Not our numbers.** They live on the endpoint's processor side, which a served-endpoint deployment owns. The client sends the image as-is; this is one of the load-bearing savings of choosing the endpoint over in-process. |

`OCR_PROMPT` (the page→Markdown transcription instruction) is **originally
authored for this project**, not copied from or read out of any vendor
source — see Licensing.

## Licensing

- **OvisOCR2 weights + code**: Apache-2.0 (Hugging Face `ATH-MaaS/OvisOCR2`).
  Passes the permissive allowlist in spirit and letter.
- **`pypdfium2`** (PDF rasterization): checked 10 August 2026 —
  `BSD-3-Clause, Apache-2.0, dependency licenses` (the bundled PDFium
  binary's own third-party components, the same permissive Chromium
  `third_party` set PDFium always ships with), actively maintained (pushed
  9 Aug 2026), repository resolves. See `plans/00l-build-vs-adopt.md`'s
  dedicated section for the full check.
- **Endpoint runtimes** (vLLM, SGLang, llama.cpp/GGUF): Apache-2.0 / MIT
  family; a deployment choice, not a dependency of `graph-owl-worker`.
- **`OCR_PROMPT`**: written for this project from the general, public shape
  of a page-transcription instruction, never read from or copied out of any
  vendor's model card or prompt library — per `plans/00i-licensing.md` rule
  3, every model-operating constant here carries a stated reason rather than
  "the reference used this."

## Slices (RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR)

Every slice was one committable increment; the workspace stayed green
throughout. Python-side TDD with `pytest`; the worker's existing
`RecordingClient`-style fakes are the testing pattern, extended in Slice 2
with a real local `http.server` double (no mocks) for `EndpointOcrModel` and
the CLI wiring.

### Slice 1 — `OvisOcrParser` assembly (pure, no model) — shipped 9 August 2026

The parser splits into three pure pieces so the network-shaped one is the
only one not unit-tested: `bytes → PIL.Image` (typed `ParseError` on
undecodable bytes), `OcrModel.parse_images` (injected) → Markdown list,
Markdown → `ParsedDocument` with page-per-section byte offsets, img-tag
filtering, repeat-cleaning.

**Closing notes**: full `mutmut` run over `graph_owl_worker/ocr.py` — 109
mutants, 107 killed, 2 survived. Both survivors are provably equivalent
(verified by a 20,000-case differential run, not by inspection alone):

- `range(MAX_PERIOD, MIN_PERIOD - 1, -1)` → `MIN_PERIOD - 2`: adds period 0
  to the scan. With period 0, `text[-0:]` is the whole text and
  `text[index:index]` is always `""`, so `unit` can never match — the extra
  iteration is a strict no-op.
- `index - period >= 0` → `index + period >= 0`: the walk only diverges when
  `index` goes negative, and at that point `text[index - period:index]` is
  an empty slice, which can never equal a non-empty `unit`. Behavior is
  identical for every reachable input.

These two mutants persist unchanged through Slices 2 and 3 (the code they
mutate was never touched again) and are re-confirmed equivalent in every
subsequent mutation run below.

### Slice 2 — `EndpointOcrModel` + CLI wiring — shipped 10 August 2026

`EndpointOcrModel`: stdlib `urllib.request`, one POST per page to
`/v1/chat/completions`, `data:` image URL, `temperature=0.0`,
`max_tokens=16384`. Non-2xx (`HTTPError`) and unreachable (`URLError`) both
map to typed `OcrError` naming the endpoint and page; a non-JSON or
wrong-shaped `200` response does too, rather than a raw `KeyError`/
`JSONDecodeError` escaping. `--ocr` flag with the deferred-import
discipline (`EXIT_UNUSABLE` + the "install the `ovis-ocr2` extra" message);
`--ocr-endpoint`/`--model`/`--prompt-file` flags; `worker.py::MEDIA_TYPES`
gained the four image extensions; `pyproject.toml`'s `ovis-ocr2` extra
gained `pypdfium2` (moved up from the Slice-2 slot the reference design
originally gave it, since the rasterizer is genuinely Slice 3's dependency,
not Slice 2's).

**A real gap found and fixed, not designed in advance**: `OvisOcrParser.parse`
called `self._model.parse_images([image])` with no exception handling —
`OcrModel.parse_images` can raise `OcrError`, and `Worker.process` only
catches `ParseError`/`UnsupportedMediaType`, so an `OcrError` would have
escaped uncaught and crashed an entire batch run over one endpoint hiccup —
the exact failure a corrupt PDF is already protected against. Fixed by
wrapping the model call and re-raising as `ParseError`, extracted as a
shared `_run_ocr` helper both `OvisOcrParser` and (Slice 3's) `OcrPdfParser`
use, so the isolation guarantee cannot silently regress in one parser and
not the other.

**Testing**: `EndpointOcrModel` and the CLI's `--ocr` wiring are both tested
against real local `http.server` doubles — no mocks — mirroring this
project's own "a real server, not a placeholder" precedent, applied to
Python for the first time in this worker (`workers/python/tests/test_ocr.py`,
`workers/python/tests/test_cli.py`). The CLI double is built per-server via a
closure rather than shared class state, because the end-to-end CLI tests run
two servers concurrently (the graph-owl double and the OCR double) — a
single scripted-class pattern would have let the second `with` block
silently clobber the first server's routes.

### Slice 3 — `OcrPdfParser` (scanned-PDF rasterization) — shipped 10 August 2026

`pypdfium2` renders each PDF page to a `PIL.Image` at `--ocr-dpi`
(`scale = dpi / 72`, since PDF canvas units are 1/72 inch), then the
identical `OcrModel` path Slice 1 built. A corrupt PDF → typed `ParseError`
(mirrors `PdfParser`). `pyproject.toml`'s `ovis-ocr2` extra now carries both
`Pillow` and `pypdfium2` — checked against `plans/00l-build-vs-adopt.md`'s
licence/maintenance gate before adopting (BSD-3-Clause/Apache-2.0, actively
maintained; see that document's dedicated section).

**A segfault found by mutation testing, not by a test**: `cargo mutants`'
Python counterpart `mutmut` mutated `scale = self._dpi / 72` to
`self._dpi * 72`. At the default `dpi=200` that is a scale factor of 14400 —
pypdfium2 asked its native PDFium binding to rasterize a canvas thousands of
times too large, and the whole mutation-testing process **segfaulted**
rather than the mutant reporting MISSED or CAUGHT. Fixed by extracting the
conversion into its own pure function, `_dpi_to_scale`, with a direct unit
test (`_dpi_to_scale(72) == 1.0`, etc.) that kills the arithmetic mutant
without ever calling the renderer — protecting the test suite itself from a
native-library crash, not just documenting the ratio. The segfault is gone
in every subsequent run.

**Closing notes**: full `mutmut` run over `graph_owl_worker/ocr.py`
(Slices 1–3 combined, the file `mutmut` is scoped to) — **245 mutants, 237
killed, 8 survived, all individually verified equivalent**:

- The 2 carried over from Slice 1 (see above).
- `.encode("utf-8")` → `.encode("UTF-8")` and `.decode("ascii")` →
  `.decode("ASCII")`: Python codec names are case-insensitive
  (`codecs.lookup("utf-8") == codecs.lookup("UTF-8")`, verified directly).
- `urllib.request.Request(..., method="POST")` → `method=None` or the kwarg
  dropped entirely: `Request.get_method()` returns `"POST"` whenever `data`
  is not `None`, and `data` is always set in this call site — verified
  directly against `urllib.request.Request`.
- `headers={"content-type": ...}` → `headers={"CONTENT-TYPE": ...}`:
  `Request.add_header` applies `.capitalize()` to every header key before
  storing it, so `"content-type"` and `"CONTENT-TYPE"` both normalize to the
  identical wire header name `"Content-type"` — no request a test could
  inspect ever differs.
- `image.save(buffer, format="PNG")` → `format="png"`: PIL's format
  selection is case-insensitive — verified directly (`img.save(..., "PNG")`
  and `img.save(..., "png")` produce byte-identical output).

No new survivor was introduced by `_run_ocr`, `OcrPdfParser`, or
`_dpi_to_scale` beyond these 8 — every one of them is a genuine equivalent
mutant, individually reasoned through and (where the reasoning depended on a
runtime fact rather than pure logic) verified by direct interpreter checks,
not asserted from inspection alone.

### Slice 4 — `scripts/verify-ocr-worker.sh` (GPU-free end-to-end) — shipped 10 August 2026

Follows `verify-sdks.sh`'s shape: real Postgres + a real `graph-owl-server`,
the worker installed into a throwaway venv and run as the actual
`graph-owl-worker` console script, pointed at `scripts/ocr-check-endpoint.py`
— a scripted, stdlib-only `http.server` model double, not a mock — plus the
committed `workers/python/tests/fixtures/page.png` (a real tiny PNG) and
`scan.pdf` (a real, synthetically-generated two-page textless PDF, built
with `pypdfium2` itself so it carries no vendor content).

**Assertions, all real**:
- A subject asset is created via `POST /assets` (`kind: "service"`, so it
  needs no parent — the cheap root-kind fixture this project's own gotchas
  already document).
- The worker run over the fixture directory reaches `POST /extraction/runs`
  for both the PNG and the PDF (as well as the pre-existing `runbook.md`
  fixture, parsed by the always-on `TextParser`).
- `GET /extraction/queue` — `graph:extraction`'s review surface — shows at
  least one claim, proving the OCR'd text (which the scripted endpoint makes
  mention the created asset's name) actually reached the graph.
- A second run of the same fixtures reports `alreadyExtracted` for every
  document, proving fingerprint pinning holds through the real HTTP/JSON
  round trip, not just in a unit test.
- The script exits non-zero on any assertion failure; no GPU, no real model,
  no network to a real endpoint anywhere in it.

**A real bug found by running the script, not by inspection**: the first
attempt named the fixture asset `prod.ocr-check-$$` (dotted, mirroring the
worker's own test fixtures' FQN style). `POST /assets` returned `400`: *"name
segment 0 ('prod.ocr-check-...') contains '.', which would make the
fully-qualified name ambiguous."* A root-kind (`service`) asset's `name`
*is* its fully-qualified name, and the server refuses a `.` in it for
exactly the reason the error says — found only by running the real
validation, not by reading the DTO. Fixed by using a dot-free fixture name
(`ocr-check-$$`).

**Real-model smoke test** (manual, never CI, not built as part of this
plan): a `--model` smoke against a real served endpoint on a GPU box,
asserting one real page parses to sensible Markdown. This is an explicit,
documented gate that waits on GPU availability, not a skipped test — the
scripted end-to-end path above is what ships and runs in CI; the smoke test
is a "when a GPU is available" checklist item, unchanged from the original
design.

## Closing documentation

- [x] `plans/21-document-ingestion.md`: crossed off the deferred
      *"Layout-model-based parsing (vision models)"* line, pointing here.
- [x] `plans/00m-capability-mapping.md`: row **OCR & Layout Analysis** —
      `PARTIAL` → `COVERED`.
- [x] `workers/README.md`: documents the `--ocr` flag, the `ovis-ocr2`
      extra, the endpoint expectation (OpenAI-compatible
      `/v1/chat/completions`, model name, no GPU required by the worker
      itself), and that OCR is a parser replacement — extraction stays the
      deterministic rule-based `MentionExtractor`, untouched.
- [x] This plan, promoted from the gitignored
      `.claude/docs/referencePlans/markdown/26-ovis-ocr2-document-parser.md`
      into a committed, tracked location.

## Explicitly out of scope (named, not silent)

- **In-process vLLM** (`InProcessOcrModel`): a small slice behind the same
  seam if a deployment needs it; not built now (see "Why a served
  endpoint").
- **LLM-based claim extraction**: OvisOCR2 replaces the *parser*; the
  extractor stays rule-based. An LLM extractor is a separate worker change
  with its own confidence-band review, per `workers/README.md` ("an LLM
  extractor replaces that one class and nothing else").
- **Owner / glossary-term resolution** for extracted claims: already
  explicitly deferred in Epic 21 Slice G; unchanged.
- **Images as first-class catalog assets**: `POST /ingest` for images is not
  this plan; source retention via `capturedAs` already covers the
  document's text.
- **OCR inside the binary, or as an MCP tool**: `00j`'s flip rule is applied
  as written — heavy lifting behind the ingestion API; MCP keeps calling
  the graph.

## Acceptance criteria — all met

- [x] `--ocr` parses `image/png`, `image/jpeg`, `image/webp`, and scanned
      `application/pdf` through the identical downstream pipeline.
- [x] Byte-offset spans survive round-trip; the fingerprint is pinned to the
      post-clean text on both sides.
- [x] `graph-owl-worker` starts without the `ovis-ocr2` extra and handles
      text/markdown/text-layer-PDF unchanged; requesting `--ocr` without the
      extra is a `2` with an installer message.
- [x] A scripted end-to-end run against a real server records an extraction
      run and re-runs idempotently — all with no GPU
      (`scripts/verify-ocr-worker.sh`).
- [x] Every model-output post-processor is covered by mutation-killed tests;
      every number in this plan has the stated reason above.
- [x] Closing docs updated (`21-`, `00m`, `workers/README.md`).
