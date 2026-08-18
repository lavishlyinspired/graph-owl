/** Epic 21's extraction claims as a `QueueConfig` — the evidence renderer
 *  that is this slice's own named RED test: a claim shown without the
 *  source sentence it came from is unreviewable, and a reviewer who
 *  cannot check provenance approves everything, which launders machine
 *  output as human-verified. `splitPassage` (pure, unit-tested) does the
 *  actual highlighting; this file is only the composition.
 *
 *  **"Edit" is deliberately not offered here.** The backend's third
 *  outcome accepts a full corrected `{subject, predicate, object}` triple,
 *  not one string — `ReviewQueue.tsx`'s `withText` action shape collects
 *  exactly one. A three-field correction form is a real, different UI,
 *  not a config-level variation of `withText`, and adding a fourth action
 *  kind to fit it would break the same claim `resolutionQueue.tsx` and
 *  `driftQueue.tsx` both hold to. Recorded as scope, not silently
 *  dropped — Accept and Reject alone are already what this slice's own
 *  acceptance criterion (the source passage, highlighted) is about.
 *
 *  **This queue has no `confirmed`/`rejected` tab.** `GET
 *  /extraction/queue` has no status filter and no envelope at all — a
 *  bare array, always the pending-equivalent set, with no way to ask for
 *  decided history through this route. One status tab, not three.
 *
 *  **No 409 on a second decision, unlike every other queue here** —
 *  `decide_extraction_claim` is an unconditional update, so a claim
 *  decided twice silently keeps the second decision rather than refusing
 *  the redecision. `ReviewQueue.tsx`'s conflict handling still applies if
 *  the server ever starts returning one; it just never does today. Not
 *  fixed here — this is a backend gap, not a frontend one. */

import { Space, Typography } from "./../../components/ui/antd-compat";
import { api, type PendingClaim } from "../../api";
import { splitPassage } from "./passageSpan";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text, Paragraph } = Typography;

const COPY = {
  label: "Extraction claims",
  subtitle: "Machine-extracted facts below the auto-assert confidence band.",
  emptyTitle: "Nothing to review",
  emptyDescription: "No extracted claims are waiting for a decision.",
  acceptAction: "Accept",
  rejectAction: "Reject",
  rejectModalTitle: "Reject this claim",
  rejectHint:
    "A rejection has to say why — without a reason, nobody reviewing this later can tell what the extractor got wrong.",
  rejectPlaceholder: "Why isn't this claim correct?",
  passageLabel: "Source passage",
};

function toQueueEntry(claim: PendingClaim): QueueEntry {
  return {
    id: claim.id,
    status: "pending",
    summary: `${Math.round(claim.confidence * 100)}% confidence`,
    detail: `${claim.subject} ${claim.predicate} ${claim.object}`,
  };
}

export function extractionQueue(): QueueConfig {
  const raw = new Map<string, PendingClaim>();

  return {
    key: "extraction",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [{ key: "pending", label: "Pending" }],
    actions: [
      { key: "accept", label: COPY.acceptAction, kind: "instant", doneMessage: "Accepted." },
      {
        key: "reject",
        label: COPY.rejectAction,
        kind: "withText",
        danger: true,
        textModalTitle: COPY.rejectModalTitle,
        textModalHint: COPY.rejectHint,
        textPlaceholder: COPY.rejectPlaceholder,
        doneMessage: "Rejected.",
      },
    ],
    emptyTitle: () => COPY.emptyTitle,
    emptyDescription: () => COPY.emptyDescription,
    async fetchEntries() {
      const claims = await api.extractionQueue();
      raw.clear();
      for (const claim of claims) raw.set(claim.id, claim);
      return claims.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const claim = raw.get(entry.id);
      if (!claim) return null;
      const { before, highlighted, after } = splitPassage(claim.passage, claim.span);
      return (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Text strong>
              {claim.subject} {claim.predicate} {claim.object}
            </Text>
          </div>
          <div>
            <Text strong>{COPY.passageLabel}</Text>
            <Paragraph style={{ marginTop: 4 }}>
              {before}
              <Text mark>{highlighted}</Text>
              {after}
            </Paragraph>
          </div>
        </Space>
      );
    },
    async performAction(entry, actionKey, text) {
      return performAsAction(async () => {
        if (actionKey === "accept") return api.decideExtractionClaim(entry.id, { outcome: "accept" });
        if (actionKey === "reject") {
          return api.decideExtractionClaim(entry.id, { outcome: "reject", reason: text ?? "" });
        }
      });
    },
  };
}
