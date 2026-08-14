/** The reasoner's conclusions about one subject, and the derivation under each
 *  — extracted from `App.tsx` by Plan 113 Slice D.
 *
 *  **Moved because of who needs to import it, not for tidiness.**
 *  `SubjectExplorer` is imported *by* `App.tsx` (through the findings queue's
 *  `ClickableSubject`), so a `SubjectExplorer` that reached back into
 *  `App.tsx` for `ReasoningView` would close an import cycle. Neither of these
 *  two components' dependencies live in `App.tsx` — they come from `api`,
 *  `theme`, `trust/TrustComponents` and `governance/*` — so lifting them out
 *  costs nothing and both callers import downward. */

import { useEffect, useState } from "react";
import { Alert, Button, Card, Flex, Space, Spin, Tag, Typography } from "./../components/ui/antd-compat";
import { ApiError, api } from "../api";
import { palette } from "../theme";
import { DerivationBadge, ProvenanceLabel } from "../trust/TrustComponents";
import { localName } from "../governance/queue";
import {
  type Explanation,
  type Row as ChainRow,
  depthOf,
  flatten,
  rulesUsed,
} from "../governance/explanation";

const { Text, Paragraph } = Typography;

/** Externalized because this file is new — the identical strings were
 *  grandfathered inside `App.tsx`, and moving them made the console's own
 *  `local/no-raw-jsx-text` rule apply for the first time. Wording unchanged. */
const COPY = {
  loadFailed: "could not load conclusions",
  emptyLead:
    "The reasoner has concluded nothing about this subject. Either no rule applies, or no run has happened since the facts that would trigger one — run reasoning from",
  emptyWhere: "Governance",
  /** Its own entry so the sentence ends outside the bolded run without a
   *  bare literal sitting in JSX. */
  fullStop: ".",
  conclusionsTitle: "These are conclusions, not assertions",
  conclusionsBody:
    "Nobody stated them. They live in their own graph and are replaced on every run — open one to see what it rests on.",
  hide: "Hide",
  why: "Why?",
  unsupportedTitle: "Nothing supports this fact",
  unsupportedBody:
    "It is neither asserted nor implied by anything the reasoner can see. That is a different answer from \u201Cit is false\u201D.",
  explainFailed: "could not explain",
  route: "route",
  circular: "circular —",
  noPremise: "nothing supports this premise",
  deepSingular: "step deep",
  deepPlural: "steps deep",
};

/** What the reasoner concluded about this subject, and why — Demo 4's second
 *  half, generalized past catalog assets by Plan 113 Slice D.
 *
 *  A derived fact is **visibly marked**: `00b` decision 2 keeps conclusions in
 *  their own graph precisely so nobody mistakes one for something a person
 *  asserted, and the console has to honour that or the separation is invisible
 *  where it matters most.
 *
 *  **`subject` is an identity, not an asset id.** This component used to build
 *  its own — `1:${assetId}`, a `dsc:`-namespaced Sid over a catalog asset's
 *  UUID — which meant the one place this console shows *what follows that
 *  nobody wrote down* could only ever be opened on an asset. A GST invoice has
 *  no UUID and no `dsc:` identity, so it had no way in at all. The caller now
 *  supplies whatever identity it actually holds (a `namespace:local` string,
 *  an IRI, or the asset's own `1:{uuid}`), and `GET /reasoning/derived`
 *  resolves all three.
 */
export function ReasoningView({
  subject,
  colors,
}: {
  subject: string;
  colors: (typeof palette)["light"];
}) {
  const [facts, setFacts] = useState<{ s: string; p: string; o: string; t: number }[] | null>(
    null,
  );
  const [open, setOpen] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setFacts(null);
    api
      .derivedAbout(subject)
      .then((found) => live && setFacts(found))
      .catch((error) => {
        if (!live) return;
        setFailed(error instanceof ApiError ? error.problem.title : COPY.loadFailed);
        setFacts([]);
      });
    return () => {
      live = false;
    };
  }, [subject]);

  if (failed) return <Alert type="error" showIcon message={failed} />;
  if (facts === null) return <Spin />;

  if (facts.length === 0) {
    return (
      <Paragraph type="secondary" style={{ fontSize: 13 }}>
        {`${COPY.emptyLead} `}
        <Text strong>{COPY.emptyWhere}</Text>
        {COPY.fullStop}
      </Paragraph>
    );
  }

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <Alert
        type="info"
        showIcon
        message={COPY.conclusionsTitle}
        description={COPY.conclusionsBody}
      />
      {facts.map((fact) => {
        const key = `${fact.s}|${fact.p}|${fact.o}`;
        return (
          <Card key={key} size="small">
            <Flex justify="space-between" align="center" wrap gap={8}>
              <Space size={6} wrap>
                <DerivationBadge status="derived" />
                <Text code style={{ fontSize: 12 }}>
                  {triple(fact)}
                </Text>
              </Space>
              <Button size="small" onClick={() => setOpen(open === key ? null : key)}>
                {open === key ? COPY.hide : COPY.why}
              </Button>
            </Flex>
            <ProvenanceLabel provenance={{ t: fact.t }} />
            {open === key && (
              <div style={{ marginTop: 10 }}>
                <DerivationChain fact={fact} colors={colors} />
              </div>
            )}
          </Card>
        );
      })}
    </Space>
  );
}

/** Why a fact holds, as an indented chain — Epic 6 Slice D on screen.
 *
 *  **The point of reasoning being explainable is that somebody reads it.** A
 *  derived fact with no visible derivation is an assertion the system made up,
 *  and the reason `00a` sells explainability is that a governance decision
 *  taken on an inference nobody can check is a governance decision nobody will
 *  take.
 *
 *  The chain is rendered to the assertions underneath, not one level down: a
 *  premise that is itself derived is the interesting half.
 */
function DerivationChain({
  fact,
  colors,
}: {
  fact: { s: string; p: string; o: string };
  colors: (typeof palette)["light"];
}) {
  const [explanation, setExplanation] = useState<Explanation | null>(null);
  const [missing, setMissing] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setExplanation(null);
    setMissing(false);
    setFailed(null);
    api
      .explain(fact.s, fact.p, fact.o)
      .then((found) => live && setExplanation(found))
      .catch((error) => {
        if (!live) return;
        // A 404 means nothing supports this fact — a different statement from
        // "the server is down", and only one of them is about the data.
        if (error instanceof ApiError && error.problem.status === 404) setMissing(true);
        else setFailed(error instanceof ApiError ? error.problem.title : COPY.explainFailed);
      });
    return () => {
      live = false;
    };
  }, [fact.s, fact.p, fact.o]);

  if (failed) return <Alert type="error" showIcon message={failed} />;
  if (missing) {
    return (
      <Alert
        type="info"
        showIcon
        message={COPY.unsupportedTitle}
        description={COPY.unsupportedBody}
      />
    );
  }
  if (!explanation) return <Spin />;

  const rows = flatten(explanation);
  const depth = depthOf(explanation);
  const rules = rulesUsed(explanation);

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <Space wrap>
        {explanation.status === "asserted" ? (
          <DerivationBadge status="asserted" />
        ) : (
          <>
            <DerivationBadge status="derived" />
            {/* Depth is the one number that says whether an inference is a
                restatement or a genuine conclusion. */}
            <Text type="secondary" style={{ fontSize: 12 }}>
              {`${depth} ${depth === 1 ? COPY.deepSingular : COPY.deepPlural}`}
            </Text>
            {rules.map((rule) => (
              <Tag key={rule}>{rule}</Tag>
            ))}
          </>
        )}
      </Space>

      <div style={{ fontSize: 13 }}>
        {rows.map((row: ChainRow, index) => (
          <div
            key={`${row.depth}-${index}`}
            style={{
              // Indentation *is* the chain. A flat list of the same rows says
              // which facts took part and not how they hang together.
              paddingLeft: row.depth * 18,
              borderLeft: row.depth > 0 ? `1px solid ${colors.border}` : undefined,
              marginLeft: row.depth > 0 ? 4 : 0,
              padding: "3px 0 3px 8px",
            }}
          >
            {row.kind === "rule" ? (
              <Space size={6}>
                <Tag color="purple" style={{ marginInlineEnd: 0 }}>
                  {row.rule}
                </Tag>
                {row.route !== undefined && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {`${COPY.route} ${row.route}`}
                  </Text>
                )}
              </Space>
            ) : row.kind === "asserted" ? (
              <Space size={6}>
                <DerivationBadge status="asserted" />
                <Text code style={{ fontSize: 12 }}>
                  {row.fact ? triple(row.fact) : ""}
                </Text>
              </Space>
            ) : row.kind === "circular" ? (
              // Only reachable through a cyclic ontology. Named rather than
              // truncated, or a modelling error reads as a short chain.
              <Text type="warning" style={{ fontSize: 12 }}>
                {`${COPY.circular} ${row.fact ? triple(row.fact) : ""}`}
              </Text>
            ) : (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {COPY.noPremise}
              </Text>
            )}
          </div>
        ))}
      </div>
    </Space>
  );
}

/** A fact as a reader reads it, without the namespace codes. */
function triple(fact: { s: string; p: string; o: string }): string {
  return `${localName(fact.s)} ${localName(fact.p)} ${localName(fact.o)}`;
}
