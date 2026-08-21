/** Turning `/findings` into a queue a business admin can actually work
 *  through — GOVERN's Contradictions tab, Plan 122a A5 follow-on.
 *
 *  **Why this exists rather than reusing `lib/memory/contradictions.ts`.**
 *  That module resolves *human-declared* disagreements between two `Memory`
 *  records. A domain pack's "two sources disagree" — GSTR-2B says filed,
 *  the books say claimed — is a different shape entirely: a rule-derived
 *  `Finding`, cited to a statute, with structured evidence a heuristic
 *  produced rather than a person typed. `/findings` already answers the
 *  reviewer question this queue exists for — "what disagrees, on what
 *  evidence, under what authority" — and already has a working aggregate
 *  endpoint (`?pack=&status=`) and decision route
 *  (`/findings/{id}/decision`) that no console screen used until now.
 *
 *  (Historical note: until the backend was extended, `/assets/{id}/memories`
 *  and `/assets/{id}/contradictions` also 400'd on a graph-native subject
 *  id, which made this the *only* working option for GST-shaped data. Both
 *  now accept a subject IRI — the choice between them is about what kind of
 *  disagreement this is, not which one happens to work.) */

import type { Finding, FindingsFilter } from "./api";

export function findingsQueryString(filter: FindingsFilter): string {
  const params = new URLSearchParams();
  if (filter.pack) params.set("pack", filter.pack);
  if (filter.status) params.set("status", filter.status);
  const query = params.toString();
  return query ? `?${query}` : "";
}

/** Every pack a loaded page of findings spans, for a filter control — sorted
 *  so the list does not reorder itself between loads. */
export function packsIn(findings: readonly Finding[]): readonly string[] {
  return [...new Set(findings.map((finding) => finding.pack))].sort();
}

const STATUS_RANK: Record<Finding["status"], number> = { pending: 0, accepted: 1, rejected: 1 };

/** Pending first — the open questions a reviewer must act on — then by
 *  [`Finding.priority`] (lower is more actionable, per
 *  `graph-owl-core::finding::Finding`), then newest first. A finding with no
 *  declared priority sorts *after* every ranked one: "nobody ranked this" is
 *  not the same claim as "this is rank zero", and treating it as the latter
 *  would put an unranked finding ahead of a rule that explicitly declared
 *  itself urgent. Returns a new array — the caller's own list, often `state`,
 *  is not touched. */
export function sortForReview(findings: readonly Finding[]): readonly Finding[] {
  return [...findings].sort((a, b) => {
    const status = STATUS_RANK[a.status] - STATUS_RANK[b.status];
    if (status !== 0) return status;

    const aPriority = a.priority ?? Number.POSITIVE_INFINITY;
    const bPriority = b.priority ?? Number.POSITIVE_INFINITY;
    if (aPriority !== bPriority) return aPriority - bPriority;

    return b.detectedAt.localeCompare(a.detectedAt);
  });
}

/** `https://graph-owl.dev/packs/gst#inv-1` → `gst:inv-1` — the same
 *  IRI-shape-only derivation `graph/graphContext.ts`'s `shortenTypeIri` uses
 *  for a type, applied here to an instance subject. Never a lookup table: the
 *  console has no business naming a domain pack's own vocabulary. */
function shortenSubjectIri(iri: string): string {
  const hash = iri.lastIndexOf("#");
  const base = hash === -1 ? iri.slice(0, iri.lastIndexOf("/")) : iri.slice(0, hash);
  const local = hash === -1 ? iri.slice(iri.lastIndexOf("/") + 1) : iri.slice(hash + 1);

  const authorityAndPath = base.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "");
  if (!authorityAndPath.includes("/")) return local;

  const prefix = authorityAndPath.slice(authorityAndPath.lastIndexOf("/") + 1);
  return prefix && prefix !== local ? `${prefix}:${local}` : local;
}

/** What to show for a finding's subject. **`subjectLabel` is not reliably
 *  populated** — verified against the running deployment, every GST finding
 *  today carries `subjectLabel: null` because no `[console.labels]` entry
 *  exists for that pack yet — so falling back to a shortened IRI is the
 *  common path, not a rare edge case, and a raw
 *  `https://graph-owl.dev/packs/gst#...` string is not something a business
 *  admin can read. */
export function subjectDisplayLabel(finding: Finding): string {
  const label = finding.subjectLabel?.trim();
  return label ? label : shortenSubjectIri(finding.subject);
}
