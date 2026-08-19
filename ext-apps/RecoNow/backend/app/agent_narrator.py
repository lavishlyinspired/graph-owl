"""An agent explaining a case — with a model when one is reachable, and always.

**The agents did not use a model at all.** `agent_runtime` was a trigger bus, a
grant check and a cost counter with no model call anywhere; `ai.py` existed
with three call sites, none of them an agent.

**What this adds, and what it deliberately does not.** An agent may now call a
model, and every word it produces passes `grounding.ground_draft` before a
reader sees it — a model that invents a figure is refused and the computed
sentence used instead. The numbers are never the model's to produce.

That split follows the evidence rather than caution for its own sake: current
practice on AI in Indian tax work is consistent that models help on language
and document handling and create risk on judgement and figures. This product's
own history agrees — a fabricated "₹8.2 L sits inside the s.16(4) window"
shipped once.

**The fallback is the design, not a degradation.** With no model reachable —
the current state of this deployment, where Ollama is not running — every case
still gets its deterministic explanation. A product whose explanations vanish
when an inference server is down cannot be relied on to explain anything.
"""

from __future__ import annotations

from typing import Any, Callable

from . import case_narrative, grounding

#: What the model is asked to do. **Rephrase, never compute** — and it is given
#: every figure it could need, because a model asked to explain a case without
#: its numbers can only invent them. Refusing what we made unavoidable would be
#: unfair to the model and useless to the reader.
PROMPT = """You are helping a chartered accountant read a GST reconciliation finding.

Rewrite the sentence below so it reads naturally for a business reader.

RULES:
- Use ONLY the figures given. Do not compute, round, or introduce any number.
- Keep every amount exactly as written.
- One or two sentences. No preamble, no markdown.

Figures for this invoice:
{figures}

Sentence to rewrite:
{sentence}
"""


def explain_case(
    case: dict[str, Any],
    *,
    model: Callable[[str], str] | None,
    log: list | None = None,
) -> dict[str, Any]:
    """The case explained, saying which produced the words.

    `model` is any callable taking a prompt and returning text — injected
    rather than imported so this is testable without an inference server, and
    so the caller decides which model an agent gets.

    The returned `source` is `"computed"` or `"model"`. A reader deciding how
    much to trust a sentence needs to know which produced it, and that is not
    something to leave implicit.
    """
    computed = case_narrative.narrate(case)
    supplied = {
        "invoice_no": case.get("invoice_no"),
        "books_amount": case.get("books_amount"),
        "portal_amount": case.get("portal_amount"),
    }

    if model is None:
        return {"text": computed, "source": "computed", "refusal": None}

    figures = "\n".join(
        f"- {key}: {value}" for key, value in supplied.items() if value is not None
    )
    try:
        drafted = model(PROMPT.format(figures=figures, sentence=computed))
    except Exception as exc:  # noqa: BLE001
        # An inference server that is down must not take the explanation with
        # it. Recorded so a run's refusals and its outages stay distinguishable.
        if log is not None:
            log.append({"refused": True, "reason": f"model unavailable: {exc}"})
        return {"text": computed, "source": "computed", "refusal": None}

    if not drafted or not drafted.strip():
        return {"text": computed, "source": "computed", "refusal": None}

    checked = grounding.ground_draft(draft=drafted, supplied=supplied, log=log)
    if not checked["grounded"]:
        return {"text": computed, "source": "computed", "refusal": checked["reason"]}
    return {"text": drafted.strip(), "source": "model", "refusal": None}


__all__ = ["PROMPT", "explain_case"]
