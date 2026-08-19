"""Explaining one case from its **real data**, with a model.

The pack's `[findings.guidance]` says what the *rule* means — generic, authored
once. `case_narrative` states the two headline figures. Neither reads the rest
of the row: the tax heads, the dates, the HSN, the place of supply, the
evidence the rule itself projected. A model can, and that is precisely the work
current practice says models are good at — reading a lot of structured context
and saying what is notable in it.

**The grounding rule still binds, and the facts are what make it usable.** A
model handed a case with no numbers can only invent them; a model handed
*every* number in the row can be specific without inventing anything. So the
supplied facts are the whole row from both sides, the identifiers, and the
derived figures a reader asks for next — the difference and its share. Any
figure outside that set is still refused, and the computed sentence shown
instead.

**Derived figures are supplied rather than left to the model.** If it has to
compute the difference it will sometimes get it wrong, and grounding will then
refuse a sentence that was only trying to be helpful. Doing the arithmetic here
means the model never has a reason to do any.
"""

from __future__ import annotations

from datetime import date, datetime
from typing import Any

#: Row fields worth putting in front of a model, in the order a reader scans
#: them. Restricted rather than "everything": a row carries internal keys that
#: mean nothing to a reviewer, and a prompt full of them buries the fields that
#: matter.
ROW_FIELDS = [
    "invoice_no",
    "invoice_date",
    "taxable",
    "igst",
    "cgst",
    "sgst",
    "cess",
    "hsn",
    "place_of_supply",
    "period",
    "itc_available",
    "reverse_charge",
    "note_type",
]


def _row_facts(row: dict[str, Any] | None) -> dict[str, Any] | None:
    if not row:
        return None
    return {k: row[k] for k in ROW_FIELDS if row.get(k) not in (None, "")}


#: Date formats a real export uses. Tried in order; the first that parses wins.
_DATE_FORMATS = ("%Y-%m-%d", "%d-%m-%Y", "%d/%m/%Y", "%Y/%m/%d")


def _days_since(raw: Any, today: str | None) -> int | None:
    """How long ago the invoice was, or `None`.

    **Supplied rather than left to the model.** A Rule 37 explanation was
    refused for stating "181 days" — the model had counted them itself, which
    is exactly the arithmetic the prompt forbids. But a time-based rule *is
    about* elapsed time, and an explanation that cannot say how long is not an
    explanation. Computing it here means the model never has a reason to.

    An unparseable date yields `None`, never a confident wrong number.
    """
    if not raw:
        return None
    reference = None
    for fmt in _DATE_FORMATS:
        try:
            reference = datetime.strptime(str(raw).strip(), fmt).date()
            break
        except ValueError:
            continue
    if reference is None:
        return None
    now = date.today()
    if today:
        for fmt in _DATE_FORMATS:
            try:
                now = datetime.strptime(today, fmt).date()
                break
            except ValueError:
                continue
    return (now - reference).days


def gather_facts(
    *,
    case: dict[str, Any],
    books_row: dict[str, Any] | None,
    portal_row: dict[str, Any] | None,
    guidance: dict[str, Any] | None,
    today: str | None = None,
) -> dict[str, Any]:
    """Everything true about this case, in one structure.

    A case with only one side still gathers: only-books findings are the most
    common kind, and refusing to explain half a row would leave them
    unexplained.
    """
    books_amount = case.get("books_amount")
    portal_amount = case.get("portal_amount")

    difference = None
    difference_pct = None
    if books_amount is not None and portal_amount is not None:
        difference = round(abs(books_amount - portal_amount), 2)
        if books_amount:
            difference_pct = round(difference / abs(books_amount) * 100, 2)

    return {
        "invoice_no": case.get("invoice_no"),
        "supplier_name": case.get("supplier_name"),
        "supplier_gstin": case.get("supplier_gstin"),
        "rule": case.get("reason_code"),
        "rule_title": (guidance or {}).get("title"),
        "rule_means": (guidance or {}).get("meaning"),
        "governed_by": case.get("governed_by"),
        "rule_summary": case.get("summary"),
        "books_tax_total": books_amount,
        "portal_tax_total": portal_amount,
        "difference": difference,
        "difference_pct": difference_pct,
        "days_since_invoice": _days_since(
            (books_row or {}).get("invoice_date") or (portal_row or {}).get("invoice_date"),
            today,
        ),
        "books": _row_facts(books_row),
        "portal": _row_facts(portal_row),
    }


def numeric_facts(facts: dict[str, Any]) -> dict[str, Any]:
    """What `grounding.ground_draft` will accept as support.

    Flattened, because grounding reads values not structure. Identifiers are
    included deliberately: an invoice number and a GSTIN contain digits, and a
    model naming them — which it must, to be useful — has to have been given
    them or the mention is refused.
    """
    supplied: dict[str, Any] = {}
    # The rule's own definition and the provision it rests on. **A statutory
    # constant is supported by the provision that states it** — 180 days, the
    # Rule 36(4) cap, 30 November. Found by running a real model: a Rule 37
    # explanation was refused for "states 180", which is not a claim about the
    # data at all. Refusing those would make every time-bound rule
    # unexplainable, which is most of the ones worth explaining.
    for key in ("rule_summary", "rule_means", "governed_by", "rule"):
        if facts.get(key):
            supplied[key] = facts[key]
    for key in (
        "invoice_no",
        "supplier_gstin",
        "books_tax_total",
        "portal_tax_total",
        "difference",
        "difference_pct",
        "days_since_invoice",
    ):
        if facts.get(key) is not None:
            supplied[key] = facts[key]
    for side in ("books", "portal"):
        for key, value in (facts.get(side) or {}).items():
            supplied[f"{side}_{key}"] = value
    return supplied


def _render_side(name: str, row: dict[str, Any] | None) -> str:
    """One side of the row, or an explicit statement that it is absent.

    **Absence is stated, never rendered as a value.** A model shown
    `portal: None` will describe it as a fact about the portal — real output
    said "the portal has not captured the tax" about an invoice the portal
    carries perfectly well.
    """
    if not row:
        return f"{name}: no matching entry on this side."
    fields = "\n".join(f"    {k}: {v}" for k, v in row.items())
    return f"{name}:\n{fields}"


def _render_total(label: str, value: Any, side_present: bool) -> str:
    """A total, or which *kind* of absence it is.

    Two absences that must not read the same: the side is missing entirely, and
    the side is present but this rule does not project a total for it. The
    first is a finding; the second is an artefact of the rule's own projection
    and means nothing about the data.
    """
    if value is not None:
        return f"    {label}: {value}"
    if not side_present:
        return f"    {label}: no entry on this side at all"
    return (
        f"    {label}: not projected by this rule — the entry exists, this rule "
        "simply does not report a total for it. Draw no conclusion from this."
    )


def build_prompt(facts: dict[str, Any]) -> str:
    """What to ask the model.

    Two things the prompt does that a naive one would not:

    - It asks about **this invoice**, not the rule. The pack already says what
      the rule means; a model repeating that adds nothing and costs a round
      trip.
    - It says the figures are **already computed** and forbids deriving new
      ones. A model told only "be accurate" will still do arithmetic; told
      there is nothing left to calculate, it has no reason to.
    """
    return f"""You are explaining one GST reconciliation finding to a chartered accountant.

Explain, for THIS invoice specifically:
- what the data actually shows on each side
- why that triggered this rule
- what is notable about the values, dates or tax heads

RULES:
- Every figure below is already computed. Do NOT calculate, derive, round or
  introduce any number that is not written here.
- Use the exact figures as given.
- Refer to this invoice, not to the rule in general — the reader already has
  the rule's definition.
- 2 to 4 sentences. Plain prose, no markdown, no bullet list, no preamble.

RULE THAT FIRED: {facts.get('rule')} — {facts.get('rule_title') or ''}
DEFINITION: {facts.get('rule_summary') or facts.get('rule_means') or ''}
GOVERNED BY: {facts.get('governed_by') or 'not stated'}

INVOICE: {facts.get('invoice_no')}
SUPPLIER: {facts.get('supplier_name') or 'not recorded'} ({facts.get('supplier_gstin') or 'no GSTIN'})

{_render_side('YOUR BOOKS', facts.get('books'))}

{_render_side('GSTR-2B / PORTAL', facts.get('portal'))}

TOTALS AND DIFFERENCES (already computed — use as written):
{_render_total('books TAX AMOUNT (igst+cgst+sgst+cess, not the taxable value)', facts.get('books_tax_total'), facts.get('books') is not None)}
{_render_total('portal TAX AMOUNT (igst+cgst+sgst+cess, not the taxable value)', facts.get('portal_tax_total'), facts.get('portal') is not None)}
{_render_total('difference between those two tax amounts', facts.get('difference'), facts.get('portal') is not None)}
{_render_total('that difference as % of the books tax amount', facts.get('difference_pct'), facts.get('portal') is not None)}
    days since the invoice date (already counted — do not count them yourself): {facts.get('days_since_invoice') if facts.get('days_since_invoice') is not None else 'not known'}

NAMING: `taxable` above is the taxable value (the invoice value before tax).
`igst`/`cgst`/`sgst`/`cess` are tax heads. Their sum is the tax amount. Do not
call one of these by the other's name — the figures are correct as given and
the labels matter as much as the numbers.
"""


__all__ = ["ROW_FIELDS", "build_prompt", "gather_facts", "numeric_facts"]
