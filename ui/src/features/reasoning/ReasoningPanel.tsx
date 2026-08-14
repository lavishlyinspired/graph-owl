/** OWL profile detection and EL classification — Plan 110 Slice 1.
 *
 *  **The capability existed and only the agent could reach it.**
 *  `graph-owl-reasoning` is 3,491 lines, `graph-owl-reasoning-el` another 947,
 *  and `POST /reasoning/el/classify`, `GET /reasoning/el/explain` and
 *  `GET /ontology/profile` had no console caller at all. Governance's own
 *  "Run reasoning" button reports "0 derived, 0 replaced" for a pack with no RL
 *  rules — correct, and it reads as broken, because nothing beside it shows
 *  what reasoning *can* do.
 *
 *  **Why this is worth more than the other unreachable routes put together.**
 *  Every finding this product surfaces is the result of a query — something
 *  asserted, or something absent. A subsumption is different in kind: a fact
 *  **nobody wrote down** that follows necessarily from the ones that were.
 *  That is the difference between a reconciliation tool and a knowledge graph,
 *  and the engine for it shipped with no way to press the button.
 *
 *  **`explain` is the half that makes it defensible.** An entailment a reviewer
 *  cannot interrogate is worse than none: it looks authoritative and cannot be
 *  checked. That is the same argument `governedBy` already makes for every
 *  finding, applied to derivation instead of citation.
 *
 *  **Domain-agnostic by construction.** A profile is a fact about an ontology's
 *  axioms, not about its subject. GST's class hierarchy, a healthcare pack's
 *  and a banking pack's are answered by the same detector, and nothing in this
 *  file names any of them. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Card, Empty, Space, Table, Tag, Typography, message } from "./../../components/ui/antd-compat";
import { ExperimentOutlined, QuestionCircleOutlined } from "@ant-design/icons";
import {
  ApiError,
  api,
  type ElClassification,
  type OntologyProfiles,
  type ProfileMembership,
} from "../../api";
import { displayTerm } from "../review/findingsQueue";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Reasoning",
  subtitle:
    "What follows from the vocabulary that nobody wrote down. Every other answer in this console is the result of a query — something asserted, or something absent. An entailment is neither: it is a fact the axioms make unavoidable.",
  profileTitle: "Profile",
  profileHint:
    "Which OWL profiles this deployment's ontology fits. A profile is a fact about the axioms, not about the subject — the same detector answers for any pack.",
  routedTo: "Reasoning is routed to",
  refusedRouting: "No profile could be routed to",
  member: "member",
  notMember: "not a member",
  violationsTitle: "What puts it outside",
  violationsHint:
    "The axiom, not just the verdict. A bare “not EL” is unactionable; the axiom that put it outside is the thing an author can change.",
  classify: "Classify (EL)",
  classifying: "Classifying…",
  classifyFailed: "Classification could not be run",
  classifyForbidden:
    "Classification is admin-only on this deployment, so this button is not available to you. The profile above is not.",
  noSidecar: "No EL reasoner is configured on this deployment",
  noSidecarBody:
    "EL classification runs in a separate process rather than inside the server, so that reasoner's licence never binds this binary. Nothing is broken and nothing is missing from your data — this deployment simply has no such process configured, and the profile detection above does not need one.",
  derivedTitle: "Derived",
  derivedHint:
    "Each row is a subsumption no document states. Ask why, and the derivation comes back as the steps that force it.",
  derivedEmpty: "Nothing was derived",
  derivedEmptyBody:
    "The ontology is consistent and its stated axioms already say everything EL can conclude from them. That is a real answer, not a failure — silence here means the vocabulary has no hidden consequences.",
  refusedTitle: "Axioms EL could not use",
  refusedHint:
    "Reported rather than skipped: a classification that quietly ignored part of the ontology would look complete and be wrong.",
  why: "Why?",
  whyTitle: "Derivation",
  whyEmpty: "No derivation — this subsumption does not hold.",
  notRun: "Not classified yet",
  notRunBody: "Classification reads the whole ontology, so it runs when you ask rather than on every page load.",
  subclass: "Class",
  superclass: "is necessarily a",
  axiom: "Axiom",
  construct: "Construct outside EL",
  reason: "Reason",
};

function Membership({ name, membership }: { name: string; membership: ProfileMembership }) {
  return (
    <Space direction="vertical" size={4} style={{ width: "100%" }}>
      <Space>
        <Text strong>{name}</Text>
        <Tag color={membership.member ? "green" : "default"}>
          {membership.member ? COPY.member : COPY.notMember}
        </Tag>
      </Space>
      {membership.violations.length > 0 && (
        <Space direction="vertical" size={0}>
          {membership.violations.slice(0, 4).map((violation) => (
            <Text key={`${violation.subject}-${violation.reason}`} type="secondary" style={{ fontSize: 12 }}>
              {`${displayTerm(violation.subject)} — ${violation.reason}`}
            </Text>
          ))}
          {membership.violations.length > 4 && (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {`…and ${membership.violations.length - 4} more`}
            </Text>
          )}
        </Space>
      )}
    </Space>
  );
}

export function ReasoningPanel() {
  const [profiles, setProfiles] = useState<OntologyProfiles | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [classification, setClassification] = useState<ElClassification | null>(null);
  const [busy, setBusy] = useState(false);
  /** Derivations already asked for, keyed by the pair. Kept rather than
   *  refetched: a reviewer comparing two entailments should not lose the first
   *  one by opening the second. */
  const [why, setWhy] = useState<Record<string, readonly string[] | "none">>({});
  /** **A deployment answer, not a failure.** The EL classifier runs as a
   *  sidecar process precisely so its reasoner's licence never binds this
   *  binary, so a deployment without one is in an ordinary state. Reporting it
   *  as "classification could not be run" sends somebody looking for a bug
   *  that is not there. */
  const [unavailable, setUnavailable] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.ontologyProfile().then(
      (found) => live && setProfiles(found),
      (error: unknown) => live && setFailure(error instanceof Error ? error.message : "unknown error"),
    );
    return () => {
      live = false;
    };
  }, []);

  const classify = useCallback(async () => {
    setBusy(true);
    try {
      setClassification(await api.classifyEl());
    } catch (error) {
      const text = error instanceof Error ? error.message : COPY.classifyFailed;
      // **The reason is in the field errors, not in the message.** RFC 9457
      // puts `detail` at the top level ("1 field failed validation") and the
      // thing that actually happened in `errors[]` — matching only the message
      // classified a configured-deployment answer as a failure.
      const fields = error instanceof ApiError ? (error.problem.errors ?? []) : [];
      const sidecar = fields.find((f) => f.field === "sidecar");
      if (sidecar) {
        setUnavailable(sidecar.detail);
      } else if (/not found|404/i.test(text)) {
        // The route is admin-gated and answers 404 to everyone else, which
        // would otherwise read as "this feature is broken" rather than "not
        // yours".
        message.error(COPY.classifyForbidden);
      } else {
        message.error(text);
      }
    } finally {
      setBusy(false);
    }
  }, []);

  const explain = useCallback(async (subclass: string, superclass: string) => {
    const key = `${subclass}|${superclass}`;
    try {
      const path = await api.explainSubsumption(subclass, superclass);
      setWhy((seen) => ({ ...seen, [key]: path }));
    } catch {
      // A 404 here is a real answer — the subsumption does not hold — not a
      // failure to report as one.
      setWhy((seen) => ({ ...seen, [key]: "none" }));
    }
  }, []);

  if (failure) return <Alert type="error" showIcon message={failure} />;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Title level={5} style={{ marginBottom: 4 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.subtitle}</Text>
      </div>

      <Card size="small" title={COPY.profileTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.profileHint}
        </Paragraph>
        {profiles ? (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Membership name="OWL 2 RL" membership={profiles.rl} />
            <Membership name="OWL 2 EL" membership={profiles.el} />
            <Membership name="OWL 2 QL" membership={profiles.ql} />
            <Alert
              type={profiles.routing.outcome === "route" ? "info" : "warning"}
              showIcon
              message={
                profiles.routing.outcome === "route"
                  ? `${COPY.routedTo} ${profiles.routing.profile}`
                  : COPY.refusedRouting
              }
              description={
                profiles.routing.outcome === "refused"
                  ? `${profiles.routing.reason} — ${displayTerm(profiles.routing.firstOffendingAxiom)}`
                  : undefined
              }
            />
          </Space>
        ) : (
          <Text type="secondary">{COPY.notRun}</Text>
        )}
      </Card>

      <Card
        size="small"
        title={COPY.derivedTitle}
        extra={
          <Button size="small" icon={<ExperimentOutlined />} loading={busy} onClick={() => void classify()}>
            {busy ? COPY.classifying : COPY.classify}
          </Button>
        }
      >
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.derivedHint}
        </Paragraph>

        {unavailable !== null ? (
          <Alert
            type="info"
            showIcon
            message={COPY.noSidecar}
            description={
              <Space direction="vertical" size={4}>
                <span>{COPY.noSidecarBody}</span>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {unavailable}
                </Text>
              </Space>
            }
          />
        ) : classification === null ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.notRun}</Text>
                <Text type="secondary">{COPY.notRunBody}</Text>
              </Space>
            }
          />
        ) : classification.subsumptions.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.derivedEmpty}</Text>
                <Text type="secondary">{COPY.derivedEmptyBody}</Text>
              </Space>
            }
          />
        ) : (
          <Table
            size="small"
            rowKey={(row) => `${row.subclass}|${row.superclass}`}
            dataSource={[...classification.subsumptions]}
            pagination={classification.subsumptions.length > 10 ? { pageSize: 10 } : false}
            scroll={{ x: "max-content" }}
            columns={[
              {
                title: COPY.subclass,
                key: "subclass",
                render: (_: unknown, row) => <Text strong>{displayTerm(row.subclass)}</Text>,
              },
              {
                title: COPY.superclass,
                key: "superclass",
                render: (_: unknown, row) => displayTerm(row.superclass),
              },
              {
                title: "",
                key: "why",
                width: 260,
                render: (_: unknown, row) => {
                  const found = why[`${row.subclass}|${row.superclass}`];
                  if (found === undefined) {
                    return (
                      <Button
                        size="small"
                        icon={<QuestionCircleOutlined />}
                        onClick={() => void explain(row.subclass, row.superclass)}
                      >
                        {COPY.why}
                      </Button>
                    );
                  }
                  if (found === "none") {
                    return (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {COPY.whyEmpty}
                      </Text>
                    );
                  }
                  return (
                    <Space direction="vertical" size={0}>
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        {COPY.whyTitle}
                      </Text>
                      {found.map((step, index) => (
                        <Text key={`${step}-${index}`} style={{ fontSize: 12 }}>
                          {`${index + 1}. ${displayTerm(step)}`}
                        </Text>
                      ))}
                    </Space>
                  );
                },
              },
            ]}
          />
        )}

        {classification !== null && classification.refusedAxioms.length > 0 && (
          <div style={{ marginTop: 16 }}>
            <Text strong>{COPY.refusedTitle}</Text>
            <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
              {COPY.refusedHint}
            </Paragraph>
            <Table
              size="small"
              rowKey={(row) => `${row.subject}-${row.construct}`}
              dataSource={[...classification.refusedAxioms]}
              pagination={false}
              columns={[
                {
                  title: COPY.axiom,
                  key: "subject",
                  render: (_: unknown, row) => displayTerm(row.subject),
                },
                {
                  title: COPY.construct,
                  key: "construct",
                  render: (_: unknown, row) => <Tag>{row.construct}</Tag>,
                },
              ]}
            />
          </div>
        )}
      </Card>
    </Space>
  );
}
