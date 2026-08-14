/** "How is this connected to that?" — Plan 111 Slice A's console half.
 *
 *  **The capability existed and stopped at the engine.**
 *  `TraversalEngine::shortest_path` and `all_paths` have been implemented and
 *  integration-tested since Epic 7a, and until Plan 111 no facade method, no
 *  route and no screen called either. Plan 110 named that defect for routes
 *  — *a capability that only one caller can reach is not a capability* — and
 *  this is the same defect one layer down, where the caller count is zero.
 *
 *  **Why it is a different question from the neighbourhood above it.**
 *  `GraphExplorer` answers *what is near this*; a pattern query answers *what
 *  is asserted*. Neither answers *how are these two things related*, and that
 *  is the question whose answer nobody can predict the shape of in advance —
 *  which is precisely what makes it a graph question rather than a join.
 *
 *  **Domain-agnostic, and provably so.** Two identities, a direction, a depth
 *  and an optional list of edge names the caller supplies. Nothing here names
 *  an invoice, a patient or a counterparty, and
 *  `scripts/check-namespace-neutrality.py` fails the build if it starts to.
 *
 *  Every judgement — is the question askable, what does the answer mean, what
 *  do we call a node — lives in `graph/paths.ts` and is asserted there. This
 *  file mounts, fetches and draws. */

import { useState } from "react";
import { Alert, AutoComplete, Button, Card, Flex, Select, Space, Tag, Typography } from "antd";
import { ApiError, api, type Asset, type PathAnswer } from "../api";
import { describeAnswer, nodeLabel, whyNotRunnable } from "./paths";
import { filterParam } from "./edgeFilter";
import type { palette } from "../theme";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "How is this connected?",
  hint: "Find the route between two things, when nobody knows in advance what the route looks like. The answer is the chain of nodes in between — not just how far apart they are.",
  target: "Search for the other end",
  find: "Find the route",
  depth: "Search depth",
  direction: "Follow edges",
  routes: "Routes",
  relationships: "Relationships",
  everyRelationship: "every relationship",
  shortest: "Shortest only",
  all: "Every route",
  failed: "That question could not be answered",
  hopsLabel: (n: number) => `${n} hops`,
  nothingYet: "Pick something at the other end and the route between them appears here.",
  /** The separator between two nodes on a route. A glyph, not a word — a
   *  route with six nodes reads as a chain rather than a sentence. */
  step: "\u2192",
};

/** Depth choices, not a slider. A traversal's cost grows with depth and the
 *  server caps it at 6 regardless; offering 47 would promise a bound the
 *  server will silently refuse. */
const DEPTHS = [2, 3, 4, 6] as const;

export function PathFinder({
  seedId,
  seedName,
  asOf,
  edgeKinds,
  colors,
}: {
  seedId: string;
  seedName: string;
  asOf: string | null;
  /** Edge names the surrounding explorer has actually seen. **Passed in
   *  rather than fetched**: the explorer already walked this neighbourhood and
   *  knows them, and a second walk here purely to populate a dropdown would
   *  pay for the same query twice. Empty means no control is offered, which is
   *  the honest state for a deployment whose graph has no edges. */
  edgeKinds: readonly string[];
  colors: (typeof palette)["light"];
}) {
  const [options, setOptions] = useState<readonly Asset[]>([]);
  const [target, setTarget] = useState<Asset | null>(null);
  const [text, setText] = useState("");
  const [hops, setHops] = useState<number>(4);
  const [direction, setDirection] = useState<"outgoing" | "incoming" | "both">("both");
  const [everyRoute, setEveryRoute] = useState(false);
  /** Plan 112 Slice B. `api.findPaths` and the route have accepted
   *  `relationshipTypes` since Plan 111 Slice A and this screen never sent it,
   *  so "how is this connected" could not be narrowed to *how* you care
   *  about. Options come from the caller — `edgeKinds`, whatever the estate
   *  actually uses — never from a list compiled in here. */
  const [edgeTypes, setEdgeTypes] = useState<readonly string[]>([]);
  const [answer, setAnswer] = useState<PathAnswer | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  /** Names for the nodes we already know about. The server returns
   *  identities, not labels — resolving every intermediate node would be one
   *  request per node, which is the cost the traversal exists to avoid. */
  const names = new Map<string, string>([
    [`1:${seedId}`, seedName],
    ...(target ? ([[`1:${target.id}`, target.fullyQualifiedName]] as [string, string][]) : []),
  ]);

  const blocked = whyNotRunnable({ from: seedId, to: target?.id ?? "" });

  const lookup = async (value: string) => {
    setText(value);
    if (value.trim().length < 2) {
      setOptions([]);
      return;
    }
    try {
      const found = await api.search(value.trim());
      setOptions(found.data.filter((asset) => asset.id !== seedId));
    } catch {
      // A failed lookup leaves the list as it was rather than clearing it: an
      // emptied dropdown mid-typing reads as "nothing matches", which is a
      // different and wrong answer.
    }
  };

  const run = async () => {
    if (!target) return;
    setRunning(true);
    setFailure(null);
    try {
      setAnswer(
        await api.findPaths({
          from: seedId,
          to: target.id,
          direction,
          hops,
          maxPaths: everyRoute ? 10 : undefined,
          relationshipTypes: filterParam(edgeTypes),
          asOf,
        }),
      );
    } catch (error) {
      setFailure(error instanceof ApiError ? error.problem.title : COPY.failed);
      setAnswer(null);
    } finally {
      setRunning(false);
    }
  };

  return (
    <Card size="small" title={COPY.title} style={{ marginTop: 16 }}>
      <Paragraph type="secondary" style={{ fontSize: 12 }}>
        {COPY.hint}
      </Paragraph>

      <Flex gap={8} wrap align="center" style={{ marginBottom: 12 }}>
        <Tag color="processing">{seedName}</Tag>
        <Text type="secondary">{COPY.step}</Text>
        <AutoComplete
          style={{ minWidth: 260 }}
          value={text}
          placeholder={COPY.target}
          options={options.map((asset) => ({
            value: asset.id,
            label: asset.fullyQualifiedName,
          }))}
          onSearch={(value) => void lookup(value)}
          onSelect={(value: string) => {
            const picked = options.find((asset) => asset.id === value) ?? null;
            setTarget(picked);
            setText(picked?.fullyQualifiedName ?? value);
          }}
        />
        <Select
          size="small"
          aria-label={COPY.depth}
          value={hops}
          style={{ width: 110 }}
          onChange={setHops}
          options={DEPTHS.map((depth) => ({ value: depth, label: COPY.hopsLabel(depth) }))}
        />
        <Select
          size="small"
          aria-label={COPY.direction}
          value={direction}
          style={{ width: 130 }}
          onChange={setDirection}
          options={[
            { value: "both", label: "either way" },
            { value: "outgoing", label: "downstream" },
            { value: "incoming", label: "upstream" },
          ]}
        />
        <Select
          size="small"
          aria-label={COPY.routes}
          value={everyRoute}
          style={{ width: 150 }}
          onChange={setEveryRoute}
          options={[
            { value: false, label: COPY.shortest },
            { value: true, label: COPY.all },
          ]}
        />
        {edgeKinds.length > 0 && (
          <Select
            mode="multiple"
            allowClear
            size="small"
            aria-label={COPY.relationships}
            placeholder={COPY.everyRelationship}
            style={{ minWidth: 200 }}
            value={[...edgeTypes]}
            onChange={(next: string[]) => setEdgeTypes(next)}
            options={edgeKinds.map((kind) => ({ value: kind, label: kind }))}
          />
        )}
        <Button size="small" type="primary" loading={running} disabled={blocked !== null} onClick={() => void run()}>
          {COPY.find}
        </Button>
        {blocked && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {blocked}
          </Text>
        )}
      </Flex>

      {failure && <Alert type="error" showIcon message={failure} style={{ marginBottom: 8 }} />}

      {answer === null && !failure && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {COPY.nothingYet}
        </Text>
      )}

      {answer && (
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          {/* The summary comes before the routes, and states the depth it
              searched: "not connected" without "within N hops" is a claim
              about the whole graph rather than about what was checked. */}
          <Text strong>{describeAnswer(answer, { hops })}</Text>
          {answer.paths.map((path) => (
            <Flex
              key={path.nodes.join(">")}
              gap={6}
              wrap
              align="center"
              style={{
                border: `1px solid ${colors.border}`,
                borderRadius: 8,
                padding: "6px 10px",
              }}
            >
              {path.nodes.map((node, index) => (
                <Space key={node} size={6}>
                  {index > 0 && <Text type="secondary">{COPY.step}</Text>}
                  <Tag color={index === 0 || index === path.nodes.length - 1 ? "processing" : undefined}>
                    {nodeLabel(node, names)}
                  </Tag>
                </Space>
              ))}
            </Flex>
          ))}
        </Space>
      )}
    </Card>
  );
}
