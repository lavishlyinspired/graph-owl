/** The one shared render surface for confidence, derivation, certification,
 *  and provenance — Epic 39 Slice E.
 *
 *  Every route that shows one of these facts imports from here. A
 *  structural test (`structural.test.ts`) asserts nothing else in `src/`
 *  defines its own confidence band or asserted/derived marker — without it,
 *  Epic 40's canvas or Epic 42's queues grow their own styling and the two
 *  drift, which is exactly how a user learns to distrust the indicator. */

import { Space, Tag, Typography } from "./../components/ui/antd-compat";
import {
  describeCertification,
  describeConfidence,
  describeDerivation,
  describeProvenance,
  type CertificationInput,
  type DerivationStatus,
  type ProvenanceInput,
} from "./confidence";
import { userTextDir } from "./direction";

const { Text } = Typography;

const CONFIDENCE_COLOR: Record<string, string> = {
  assert: "green",
  surface: "gold",
  ignore: "default",
};

const CERTIFICATION_COLOR: Record<string, string> = {
  certified: "green",
  deprecated: "red",
  uncertified: "default",
};

/** A confidence value as a band, never a bare number — the symbol and label
 *  distinguish the band without relying on the tag colour alone. */
export function ConfidenceBadge({ confidence }: { confidence: number }) {
  const d = describeConfidence(confidence);
  return (
    <Tag color={CONFIDENCE_COLOR[d.band]}>
      {d.symbol} {d.label}
    </Tag>
  );
}

/** Asserted vs derived, marked identically everywhere a fact appears —
 *  `00b` decision 2. */
export function DerivationBadge({ status }: { status: DerivationStatus }) {
  const d = describeDerivation(status);
  return (
    <Tag color={status === "asserted" ? "blue" : "purple"}>
      {d.symbol} {d.label}
    </Tag>
  );
}

/** Certified / deprecated / uncertified, said plainly rather than left as a
 *  confident-looking blank when nothing is recorded. */
export function CertificationBadge({ certification }: { certification: CertificationInput }) {
  const d = describeCertification(certification);
  return (
    <Tag color={CERTIFICATION_COLOR[d.state]}>
      {d.symbol} {d.label}
    </Tag>
  );
}

/** Source, transaction, and ingestor — each rendered as given, or honestly
 *  as "not captured" rather than omitted, matching the trust bar's existing
 *  "lineage not captured" language. */
export function ProvenanceLabel({ provenance }: { provenance: ProvenanceInput }) {
  const p = describeProvenance(provenance);
  return (
    <Space size={8} wrap>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {p.sourceLabel}
      </Text>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {p.transactionLabel}
      </Text>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {p.ingestedByLabel}
      </Text>
    </Space>
  );
}

/** Re-exported so a call site needs only one import to reach every trust
 *  primitive — the base direction for a user-supplied label (a name, a
 *  description, a tag, a graph-node caption), Epic 94 Slice C. Applied as a
 *  `dir` prop directly on the element rendering the text (every antd
 *  Typography component forwards it), never as a wrapping element: wrapping
 *  a heading or paragraph in a `<span>` is invalid nesting and exactly the
 *  kind of thing Slice F's axe check exists to catch. */
export { userTextDir };
