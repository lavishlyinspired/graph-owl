/** Epic 20's drift, over HTTP, as a `QueueConfig` — declared (from a
 *  metadata-as-code push) vs. actual (the live catalog value), shown as a
 *  diff with a per-item apply. `apply`/`ignore` are symmetric here, unlike
 *  the resolution queue's confirm/reject: either one 409s on an entry
 *  that is no longer `pending`, which `ReviewQueue.tsx`'s generic conflict
 *  handling already covers with no queue-specific code. */

import { Space, Typography } from "./../../components/ui/antd-compat";
import { api, type DriftItem, type DriftStatus } from "../../api";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text } = Typography;

const COPY = {
  label: "Drift",
  subtitle: "Where the catalog's live state and its declared (metadata-as-code) state disagree.",
  emptyPendingTitle: "Nothing drifting",
  emptyPendingDescription: "Declared and live state agree on everything reported so far.",
  emptyAppliedTitle: "No drift applied yet",
  emptyAppliedDescription: "No drifted field has been written back to its declared value yet.",
  emptyIgnoredTitle: "No drift ignored yet",
  emptyIgnoredDescription: "No drifted field has been ignored yet.",
  declaredLabel: "Declared",
  liveLabel: "Live",
  fieldLabel: "Field",
  fieldSeparator: " · ",
  applyAction: "Apply declared value",
  applyConfirmTitle: "Apply the declared value?",
  applyConfirmBody: "This writes the declared value into the live catalog, overwriting what is there now.",
  ignoreAction: "Ignore",
  ignoreModalTitle: "Ignore this drift",
  ignoreHint:
    "An ignore has to say why — without a reason, nobody reviewing this later can tell whether the live value was actually intentional.",
  ignorePlaceholder: "Why should this drift stand?",
};

function displayValue(value: string | null): string {
  return value === null ? "(none)" : value;
}

function toQueueEntry(item: DriftItem): QueueEntry {
  return {
    id: item.id,
    status: item.status,
    summary: item.kind === "liveEdited" ? "Live edited" : "Declaration not applied",
    detail: `${item.fullyQualifiedName}${COPY.fieldSeparator}${item.field}`,
    decidedSummary: item.status === "pending" ? undefined : `${item.status}${item.decidedBy ? ` by ${item.decidedBy}` : ""}`,
    reason: item.reason,
  };
}

export function driftQueue(): QueueConfig {
  const raw = new Map<string, DriftItem>();

  return {
    key: "drift",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [
      { key: "pending", label: "Pending" },
      { key: "applied", label: "Applied" },
      { key: "ignored", label: "Ignored" },
    ],
    actions: [
      {
        key: "apply",
        label: COPY.applyAction,
        kind: "instant",
        confirmTitle: COPY.applyConfirmTitle,
        confirmBody: COPY.applyConfirmBody,
        doneMessage: "Applied.",
      },
      {
        key: "ignore",
        label: COPY.ignoreAction,
        kind: "withText",
        danger: true,
        textModalTitle: COPY.ignoreModalTitle,
        textModalHint: COPY.ignoreHint,
        textPlaceholder: COPY.ignorePlaceholder,
        doneMessage: "Ignored.",
      },
    ],
    emptyTitle: (status) =>
      status === "applied"
        ? COPY.emptyAppliedTitle
        : status === "ignored"
          ? COPY.emptyIgnoredTitle
          : COPY.emptyPendingTitle,
    emptyDescription: (status) =>
      status === "applied"
        ? COPY.emptyAppliedDescription
        : status === "ignored"
          ? COPY.emptyIgnoredDescription
          : COPY.emptyPendingDescription,
    async fetchEntries(status) {
      const page = await api.driftQueue({ status: status as DriftStatus });
      raw.clear();
      for (const item of page.data) raw.set(item.id, item);
      return page.data.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const item = raw.get(entry.id);
      if (!item) return null;
      return (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <div>
            <Text strong>{COPY.fieldLabel}</Text>
            <div>
              {item.fullyQualifiedName}
              {COPY.fieldSeparator}
              {item.field}
            </div>
          </div>
          <div>
            <Text strong>{COPY.declaredLabel}</Text>
            <div>{displayValue(item.declaredValue)}</div>
          </div>
          <div>
            <Text strong>{COPY.liveLabel}</Text>
            <div>{displayValue(item.liveValue)}</div>
          </div>
        </Space>
      );
    },
    async performAction(entry, actionKey, text) {
      return performAsAction(async () => {
        if (actionKey === "apply") return api.applyDrift(entry.id);
        if (actionKey === "ignore") return api.ignoreDrift(entry.id, text ?? "");
      });
    },
  };
}
