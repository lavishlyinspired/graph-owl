/** Epic 42 Slice D: the page-level wiring `App.tsx` renders for its
 *  "Review" nav item. Picking *which* queue is itself queue-specific
 *  reasoning, so it lives here, one level above `ReviewQueue.tsx`, which
 *  must stay free of exactly this kind of branch (see its own structural
 *  test) — the identical split `VocabularySection.tsx` already
 *  established one layer above `VocabularyBrowser.tsx`.
 *
 *  **Proposals (Epic 35) wired in, Phase 3 item 3.2**: `GET
 *  /change-proposals` (catalog-wide, added alongside `GET /me`) closes the
 *  gap this file's own comment used to name here. */

import { useMemo, useState } from "react";
import { Segmented, Space } from "antd";
import { ReviewQueue } from "./ReviewQueue";
import { resolutionQueue } from "./resolutionQueue";
import { extractionQueue } from "./extractionQueue";
import { driftQueue } from "./driftQueue";
import { proposalsQueue } from "./proposalsQueue";
import { alignmentQueue } from "./alignmentQueue";
import { findingsQueue } from "./findingsQueue";
import type { QueueConfig } from "./queues";
import { readParam, writeParam } from "../deepLink";

type QueueKind = "resolution" | "extraction" | "drift" | "proposals" | "alignment" | "findings";

const KIND_OPTIONS: { readonly label: string; readonly value: QueueKind }[] = [
  { label: "Merge review", value: "resolution" },
  { label: "Extraction claims", value: "extraction" },
  { label: "Drift", value: "drift" },
  { label: "Proposals", value: "proposals" },
  { label: "Alignments", value: "alignment" },
  { label: "Findings", value: "findings" },
];

function isQueueKind(value: string | null): value is QueueKind {
  return (
    value === "resolution" ||
    value === "extraction" ||
    value === "drift" ||
    value === "proposals" ||
    value === "alignment" ||
    value === "findings"
  );
}

function configFor(kind: QueueKind): QueueConfig {
  if (kind === "extraction") return extractionQueue();
  if (kind === "drift") return driftQueue();
  if (kind === "proposals") return proposalsQueue();
  if (kind === "alignment") return alignmentQueue();
  if (kind === "findings") return findingsQueue();
  return resolutionQueue();
}

export function ReviewSection() {
  const [kind, setKindRaw] = useState<QueueKind>(() => {
    const named = readParam("kind");
    return isQueueKind(named) ? named : "resolution";
  });

  const setKind = (next: QueueKind) => {
    setKindRaw(next);
    writeParam("kind", next === "resolution" ? null : next);
    writeParam("status", null);
    writeParam("entry", null);
  };

  // Every `QueueConfig` closes over its own `raw` correlation map
  // (`fetchEntries` populates it, `fetchDetail`/`performAction` read it
  // back) — a fresh `configFor` call is a fresh, empty map. Not memoizing
  // this meant any unrelated re-render higher in the tree (the header's
  // own ticking clock, for instance) silently swapped in a new, empty
  // config underneath an already-mounted `ReviewQueue`, and the very next
  // `fetchDetail` call missed its own `fetchEntries` data — found by
  // Playwright, not by inspection: the detail pane rendered blank instead
  // of throwing, so nothing failed loudly until a real browser was driven
  // through it.
  const config = useMemo(() => configFor(kind), [kind]);

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <Segmented value={kind} onChange={(value) => setKind(value as QueueKind)} options={KIND_OPTIONS} />
      <ReviewQueue key={kind} config={config} />
    </Space>
  );
}
