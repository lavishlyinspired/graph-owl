/** What this deployment believes twice, incompatibly — Plan 111 Slice C.
 *
 *  **Two routes shipped with Epic 31 and no browser has ever called either.**
 *  `GET /assets/{id}/contradictions` finds pairs; `POST /contradictions/reviews`
 *  records a verdict. The engine has been flagging conflicting institutional
 *  knowledge the whole time, into a queue nobody could open.
 *
 *  **Nothing here resolves anything, and the wording is chosen so the screen
 *  cannot imply that it does.** The engine never picks a winner, never hides
 *  either side, and keeps a *confirmed* pair in the queue — confirming a
 *  contradiction is agreeing that it exists, not settling it. Software that
 *  adjudicates institutional disagreement ends the argument without
 *  resolving it, which is worse than the disagreement.
 *
 *  **A pair with one side missing is still shown.** Recall filters by
 *  confidence, so the other memory is usually real and simply not on this
 *  page; dropping the pair would suppress the one thing this surface exists
 *  to say.
 *
 *  Domain-agnostic: a memory is an attributed claim about an entity. What the
 *  claims are about is the deployment's business, and nothing here reads it. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Card, Flex, Popconfirm, Space, Tag, Typography, message } from "./../components/ui/antd-compat";
import { ApiError, api } from "../api";
import { type Contradiction, type ResolvedPair, kindLabel, pairsFor } from "./contradictions";
import type { Memory } from "./memory";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "Open disagreements",
  hint: "Two things recorded here that cannot both be right. Nothing is hidden and nothing is auto-resolved — a person decides, and even then both stay on the record.",
  loadFailed: "Could not load disagreements",
  confirm: "Yes, they disagree",
  dismiss: "No, they don't",
  dismissTitle: "Dismiss this pair?",
  dismissBody: "It will not be flagged again. Both memories stay exactly as they are.",
  confirmed: "Recorded — the pair stays flagged, because agreeing it exists is not settling it.",
  dismissed: "Dismissed.",
  actionFailed: "That verdict could not be recorded",
  notShown: "The other side is not shown on this page — it may be below the confidence this view surfaces.",
  unknown: "not shown here",
};

function Side({ memory }: { memory: Memory | null }) {
  if (memory === null) {
    return (
      <Text type="secondary" style={{ fontSize: 13, fontStyle: "italic" }}>
        {COPY.unknown}
      </Text>
    );
  }
  return (
    <Space direction="vertical" size={2}>
      <Text style={{ fontSize: 13 }}>{memory.content}</Text>
      <Text type="secondary" style={{ fontSize: 11 }}>
        {new Date(memory.asOf).toLocaleDateString()}
      </Text>
    </Space>
  );
}

export function Disagreements({ assetId, memories }: { assetId: string; memories: readonly Memory[] }) {
  const [pairs, setPairs] = useState<readonly ResolvedPair[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [decided, setDecided] = useState<ReadonlySet<string>>(new Set());

  const load = useCallback(() => {
    api.contradictions(assetId).then(
      (found: Contradiction[]) => {
        // A route answering an unexpected shape must degrade to "nothing to
        // show" rather than unmount the whole Knowledge tab around it.
        setPairs(pairsFor(Array.isArray(found) ? found : [], memories));
        setFailure(null);
      },
      (error: unknown) => {
        setFailure(error instanceof ApiError ? error.problem.title : COPY.loadFailed);
        setPairs([]);
      },
    );
  }, [assetId, memories]);

  useEffect(load, [load]);

  const decide = async (pair: ResolvedPair, verdict: "confirmed" | "dismissed") => {
    const key = `${pair.id.a}-${pair.id.b}`;
    try {
      await api.reviewContradiction({ a: pair.id.a, b: pair.id.b, verdict });
      // Only a dismissal leaves the queue. A confirmed pair stays, and
      // removing it here would tell a lie the server does not tell.
      if (verdict === "dismissed") setDecided((seen) => new Set(seen).add(key));
      message.success(verdict === "confirmed" ? COPY.confirmed : COPY.dismissed);
    } catch (error) {
      message.error(error instanceof ApiError ? error.problem.title : COPY.actionFailed);
    }
  };

  const open = (pairs ?? []).filter((pair) => !decided.has(`${pair.id.a}-${pair.id.b}`));

  // Nothing to say when nothing disagrees. An empty card claiming a clean
  // bill of health on every asset is noise; a load failure is not silence,
  // so that still speaks.
  if (!failure && open.length === 0) return null;

  return (
    <Card size="small" title={COPY.title}>
      <Paragraph type="secondary" style={{ fontSize: 12 }}>
        {COPY.hint}
      </Paragraph>
      {failure && <Alert type="error" showIcon message={failure} />}
      <Space direction="vertical" size={10} style={{ width: "100%" }}>
        {open.map((pair) => (
          <div key={`${pair.id.a}-${pair.id.b}`}>
            <Flex justify="space-between" align="center" wrap gap={8}>
              <Tag color={pair.kind === "candidate" ? "warning" : "error"}>{kindLabel(pair.kind)}</Tag>
              <Space size={4}>
                <Button size="small" onClick={() => void decide(pair, "confirmed")}>
                  {COPY.confirm}
                </Button>
                {/* Dismissing is the one that stops the pair being flagged
                    again, so it is the one a misclick must not do silently. */}
                <Popconfirm
                  title={COPY.dismissTitle}
                  description={COPY.dismissBody}
                  onConfirm={() => void decide(pair, "dismissed")}
                >
                  <Button size="small">{COPY.dismiss}</Button>
                </Popconfirm>
              </Space>
            </Flex>
            <Flex gap={16} wrap style={{ marginTop: 6 }}>
              <div style={{ flex: "1 1 240px" }}>
                <Side memory={pair.a} />
              </div>
              <div style={{ flex: "1 1 240px" }}>
                <Side memory={pair.b} />
              </div>
            </Flex>
            {!pair.complete && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                {COPY.notShown}
              </Text>
            )}
          </div>
        ))}
      </Space>
    </Card>
  );
}
