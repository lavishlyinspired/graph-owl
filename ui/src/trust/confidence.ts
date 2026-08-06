/** What the catalog knows about a fact's trustworthiness, in one place —
 *  Epic 39 Slice E. Confidence, derivation, certification, and provenance
 *  each rendered by exactly one component set (`TrustComponents.tsx`) so a
 *  screen cannot grow its own drifting version of "what does 0.62 mean".
 *
 *  `00f-ui-architecture.md`: the part that can be wrong in a way somebody
 *  would act on belongs in a pure function, tested without rendering
 *  anything. Every descriptor here carries a distinct label *and* a distinct
 *  symbol per state — never colour alone — because a colour-only distinction
 *  is invisible to a colour-blind reader and to a printed screenshot in a
 *  review. */

export type ConfidenceBand = "assert" | "surface" | "ignore";

/** `00c-domain-model.md`: >=0.8 assert, 0.5-0.8 surface, <0.5 ignore. The
 *  boundaries are half-open on the low side — 0.8 itself assert-bands. */
export function bandOf(confidence: number): ConfidenceBand {
  if (confidence >= 0.8) return "assert";
  if (confidence >= 0.5) return "surface";
  return "ignore";
}

export interface ConfidenceDescriptor {
  readonly band: ConfidenceBand;
  readonly label: string;
  readonly symbol: string;
}

export function describeConfidence(confidence: number): ConfidenceDescriptor {
  const band = bandOf(confidence);
  const value = confidence.toFixed(2);
  switch (band) {
    case "assert":
      return { band, label: `confident (${value})`, symbol: "●" };
    case "surface":
      return { band, label: `uncertain (${value})`, symbol: "◐" };
    case "ignore":
      return { band, label: `low confidence (${value})`, symbol: "○" };
  }
}

export type DerivationStatus = "asserted" | "derived";

export interface DerivationDescriptor {
  readonly status: DerivationStatus;
  readonly label: string;
  readonly symbol: string;
}

/** `00b` decision 2: a derived fact is visibly marked, everywhere it
 *  appears, so nobody mistakes a conclusion for something a person stated. */
export function describeDerivation(status: DerivationStatus): DerivationDescriptor {
  return status === "asserted"
    ? { status, label: "Asserted", symbol: "✓" }
    : { status, label: "Derived", symbol: "∴" };
}

export interface CertificationInput {
  readonly certified?: boolean | null;
  readonly deprecated?: boolean | null;
}

export type CertificationState = "certified" | "deprecated" | "uncertified";

export interface CertificationDescriptor {
  readonly state: CertificationState;
  readonly label: string;
  readonly symbol: string;
}

/** Says plainly what is known. `uncertified` is the honest default rather
 *  than a confident-looking blank — this is not "unknown", it is "the
 *  catalog has recorded no certification for this asset". Deprecation wins
 *  over certification when both are set: a deprecated-but-certified asset is
 *  a warning to a reader before it is a badge. */
export function describeCertification(input: CertificationInput): CertificationDescriptor {
  if (input.deprecated) return { state: "deprecated", label: "deprecated", symbol: "⚠" };
  if (input.certified) return { state: "certified", label: "certified", symbol: "✓" };
  return { state: "uncertified", label: "uncertified", symbol: "—" };
}

export interface ProvenanceInput {
  readonly source?: string | null;
  readonly t?: number | null;
  readonly ingestedBy?: string | null;
}

export interface ProvenanceDescriptor {
  readonly sourceLabel: string;
  readonly transactionLabel: string;
  readonly ingestedByLabel: string;
}

/** `t` is the flake's transaction ordinal (`graph-owl-core::Flake::t`), not
 *  a wall-clock timestamp — rendered as the ordinal itself rather than
 *  guessed into a date, which would be a fabricated fact wearing a real
 *  one's clothes. */
export function describeProvenance(input: ProvenanceInput): ProvenanceDescriptor {
  return {
    sourceLabel: input.source ?? "source not captured",
    transactionLabel: input.t == null ? "time not captured" : `as of t${input.t}`,
    ingestedByLabel: input.ingestedBy ?? "ingested by not captured",
  };
}
