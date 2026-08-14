/** Epic 42 Slice F: agent sessions (identity, capabilities, scope) and their
 *  operation history — filterable by type and entity, with write-backs
 *  (`applied`) visually distinguished from everything that did not change
 *  the catalog (`proposed`, `refused`).
 *
 *  **Decision 5, read-only by construction, not by a runtime check**: this
 *  file makes no mutating request — no revoke, no grant edit — only
 *  `api.agentGrants()` and `api.agentActivity()`, both `GET`.
 *  `AgentActivityPanel.structural.test.ts` greps this file's own raw source
 *  for any non-GET verb and fails the build if one appears, the same
 *  mechanism `VocabularyBrowser.structural.test.ts` already established for
 *  a different invariant. */

import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useMemo, useState } from "react";
import { Alert, Empty, Input, Select, Space, Spin, Table, Tag, Typography } from "./../../components/ui/antd-compat";
import { api, type AgentActivity, type AgentCapability, type AgentGrant } from "../../api";
import { readParam, writeParam } from "../deepLink";
import { describeCapability, describeOutcome, filterActivity } from "./agentActivity";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Agent activity",
  intro: "Every grant, and what each agent has done with it.",
  loading: "Loading…",
  loadError: "Could not load agent grants.",
  noGrants: "No agent has been granted anything yet.",
  grantsTitle: "Agents",
  selectAgent: "Select an agent to see its history.",
  activityTitle: "History",
  activityLoadError: "Could not load this agent's history.",
  activityEmpty: "No activity recorded for this agent yet.",
  activityFilteredEmpty: "No activity matches these filters.",
  filterCapability: "Filter by type",
  filterEntity: "Filter by entity",
  allOutcomes: "All outcomes",
  writeBack: "Write-back",
};

function outcomeColor(outcome: AgentActivity["outcome"]): string {
  switch (outcome) {
    case "applied":
      return "green";
    case "proposed":
      return "blue";
    case "refused":
      return "red";
  }
}

export function AgentActivityPanel() {
  const [grants, setGrants] = useState<AgentGrant[] | null>(null);
  const [grantsError, setGrantsError] = useState<string | null>(null);
  // The one surface in this epic missing the `readParam`/`writeParam`
  // convention every sibling surface (`ReviewQueue`, the asset explorer's
  // `?asset=`) already uses — `?agent=<id>` so a shared link opens
  // straight into the same agent's history.
  const [selected, setSelectedRaw] = useState<string | null>(() => readParam("agent"));
  const [activity, setActivity] = useState<AgentActivity[] | null>(null);
  const [activityError, setActivityError] = useState<string | null>(null);
  const [capabilityFilter, setCapabilityFilter] = useState<AgentCapability | null>(null);
  const [outcomeFilter, setOutcomeFilter] = useState<AgentActivity["outcome"] | null>(null);
  const [entityFilter, setEntityFilter] = useState("");

  const setSelected = (id: string | null) => {
    setSelectedRaw(id);
    writeParam("agent", id);
  };

  useEffect(() => {
    api
      .agentGrants()
      .then((g) => setGrants(g))
      .catch((e: unknown) => setGrantsError(e instanceof Error ? e.message : COPY.loadError));
  }, []);

  useEffect(() => {
    if (!selected) {
      setActivity(null);
      return;
    }
    let cancelled = false;
    setActivity(null);
    setActivityError(null);
    api.agentActivity(selected).then(
      (page) => {
        if (!cancelled) setActivity(page.data);
      },
      (e: unknown) => {
        if (!cancelled) setActivityError(e instanceof Error ? e.message : COPY.activityLoadError);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [selected]);

  const filtered = useMemo(
    () =>
      activity
        ? filterActivity(activity, {
            capability: capabilityFilter,
            outcome: outcomeFilter,
            entity: entityFilter,
          })
        : [],
    [activity, capabilityFilter, outcomeFilter, entityFilter],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {grantsError && <Alert type="error" showIcon message={COPY.loadError} description={grantsError} />}

      {grants === null ? (
        <Spin />
      ) : grants.length === 0 ? (
        <Empty description={<Text>{COPY.noGrants}</Text>} />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Table
            size="small"
            rowKey={(grant: AgentGrant) => grant.id}
            dataSource={grants}
            pagination={false}
            onRow={(grant: AgentGrant) => ({
              onClick: () => setSelected(grant.agent.id),
              // A whole-row click target has no default keyboard equivalent
              // — confirmed directly against a real page (`tabindex: null`
              // on the rendered `<tr>`), not assumed. `tabIndex={0}` plus
              // Enter/Space makes the row an operable widget.
              //
              // **`role="button"` was tried first and reverted**: an
              // explicit `role` *replaces* an element's implicit one rather
              // than adding to it, so it silently turned every `<tr>` into
              // a button with no row semantics at all — breaking the
              // table's own accessibility structure, and, mechanically,
              // this file's own Playwright spec's `getByRole("row", ...)`
              // locator. `aria-selected` is the correct semantics for a
              // selectable row and leaves the implicit `row` role intact.
              tabIndex: 0,
              "aria-selected": selected === grant.agent.id,
              onKeyDown: (event: ReactKeyboardEvent) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  setSelected(grant.agent.id);
                }
              },
              style: { cursor: "pointer" },
            })}
            columns={[
              {
                title: "Agent",
                key: "agent",
                render: (_: unknown, grant: AgentGrant) => (
                  <Text strong={selected === grant.agent.id}>{grant.agent.displayName}</Text>
                ),
              },
              {
                title: "Capabilities",
                key: "capabilities",
                render: (_: unknown, grant: AgentGrant) => (
                  <Space size={[4, 4]} wrap>
                    {grant.capabilities.map((c) => (
                      <Tag key={c}>{describeCapability(c)}</Tag>
                    ))}
                  </Space>
                ),
              },
              {
                title: "Scope",
                key: "scope",
                render: (_: unknown, grant: AgentGrant) => (
                  <Text type="secondary">{grant.scope ? grant.scope.fqnPrefix : "whole estate"}</Text>
                ),
              },
              {
                title: "Rate limit",
                key: "rateLimit",
                render: (_: unknown, grant: AgentGrant) => (
                  <Text type="secondary">
                    {`${grant.rateLimit.maxWrites} / ${grant.rateLimit.windowSeconds}s`}
                  </Text>
                ),
              },
            ]}
          />

          {!selected ? (
            <Paragraph type="secondary">{COPY.selectAgent}</Paragraph>
          ) : (
            <Space direction="vertical" size="small" style={{ width: "100%" }}>
              <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
                {COPY.activityTitle}
              </Title>
              <Space wrap>
                <Select
                  allowClear
                  placeholder={COPY.filterCapability}
                  aria-label={COPY.filterCapability}
                  style={{ minWidth: 180 }}
                  value={capabilityFilter ?? undefined}
                  onChange={(v) => setCapabilityFilter((v as AgentCapability | undefined) ?? null)}
                  options={(
                    [
                      "proposeDescription",
                      "proposeTags",
                      "proposeOwner",
                      "applyDescription",
                      "applyTags",
                      "recordMemory",
                      "recordInvestigation",
                      "createGlossaryTerm",
                      "createQualityTest",
                      "linkLineage",
                    ] as const
                  ).map((c) => ({ value: c, label: describeCapability(c) }))}
                />
                <Select
                  allowClear
                  placeholder={COPY.allOutcomes}
                  aria-label={COPY.allOutcomes}
                  style={{ minWidth: 140 }}
                  value={outcomeFilter ?? undefined}
                  onChange={(v) =>
                    setOutcomeFilter((v as AgentActivity["outcome"] | undefined) ?? null)
                  }
                  options={[
                    { value: "applied", label: "Applied" },
                    { value: "proposed", label: "Proposed" },
                    { value: "refused", label: "Refused" },
                  ]}
                />
                <Input
                  placeholder={COPY.filterEntity}
                  value={entityFilter}
                  onChange={(e) => setEntityFilter(e.target.value)}
                  style={{ width: 220 }}
                  allowClear
                />
              </Space>

              {activityError && (
                <Alert type="error" showIcon message={COPY.activityLoadError} description={activityError} />
              )}
              {activity === null ? (
                <Spin />
              ) : filtered.length === 0 ? (
                <Paragraph type="secondary">
                  {activity.length === 0 ? COPY.activityEmpty : COPY.activityFilteredEmpty}
                </Paragraph>
              ) : (
                <Table
                  size="small"
                  rowKey={(entry: AgentActivity) => entry.id}
                  dataSource={filtered}
                  pagination={false}
                  columns={[
                    {
                      title: "Type",
                      key: "capability",
                      render: (_: unknown, entry: AgentActivity) => describeCapability(entry.capability),
                    },
                    {
                      title: "Outcome",
                      key: "outcome",
                      render: (_: unknown, entry: AgentActivity) => {
                        const described = describeOutcome(entry);
                        return (
                          <Space direction="vertical" size={0}>
                            <Tag color={outcomeColor(entry.outcome)}>
                              {described.label}
                              {described.isWriteBack ? ` — ${COPY.writeBack}` : ""}
                            </Tag>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {described.detail}
                            </Text>
                          </Space>
                        );
                      },
                    },
                    {
                      title: "Entity",
                      key: "targetFqn",
                      render: (_: unknown, entry: AgentActivity) => <Text code>{entry.targetFqn}</Text>,
                    },
                    {
                      title: "When",
                      key: "at",
                      render: (_: unknown, entry: AgentActivity) => (
                        <Text type="secondary">{new Date(entry.at).toLocaleString()}</Text>
                      ),
                    },
                  ]}
                />
              )}
            </Space>
          )}
        </Space>
      )}
    </Space>
  );
}
