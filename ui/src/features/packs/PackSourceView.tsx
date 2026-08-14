/** The source's own data, opened in Explore — Plan 115 Slice B2.
 *
 *  B1 made an imported source listable in the sider; this makes it inspectable
 *  in the Explore content pane, without bouncing the CA out of the section
 *  they were in — the original click dumped them into the Reconciliation.
 *
 *  Reads the source's graph the way the block does: by asking the graph what
 *  it holds, scoped to `source.iri` (the IRI the wire itself reported, not a
 *  name the UI re-assembled), and naming subjects by the graph's own local
 *  names. A subject opens its neighbourhood through `SubjectExplorer` — the
 *  same walk Plan 113 Slice C built for anything that has no catalog row.
 *  The Reconciliation stays one deliberate action away, never the destination
 *  a click lands on by default. */

import { useEffect, useState } from "react";
import { Alert, Button, Card, Space, Table, Tag, Typography } from "antd";
import { api } from "../../api";
import { palette } from "../../theme";
import { SubjectExplorer } from "../../graph/SubjectExplorer";
import {
  subjectsFromSparql,
  subjectsQuery,
  typesQuery,
  type LoadedSource,
  type SourceSubject,
} from "./packData";

const { Paragraph, Text, Title } = Typography;

const COPY = {
  hint: "Everything this source imported, as the graph itself asserts it. A subject opens its neighbourhood.",
  failed: "Could not read this source's graph",
  truncated: "The graph holds more subjects than one read allows — these are the first it returned.",
  name: "Name",
  kind: "Kind",
  triples: "Triples",
  none: "This source imported no subjects.",
  back: "Back to subjects",
  triplesOf: (n: number) => `${n} triple${n === 1 ? "" : "s"}`,
  reconcile: "Work this period in Reconciliation",
  emptyKind: "—",
};

export function PackSourceView({
  source,
  packLabel,
  colors = palette.light,
  onReconcile,
}: {
  source: LoadedSource;
  packLabel: string;
  colors?: (typeof palette)["light"];
  onReconcile: () => void;
}) {
  const [subjects, setSubjects] = useState<readonly SourceSubject[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState<SourceSubject | null>(null);

  useEffect(() => {
    let live = true;
    setSubjects(null);
    setTruncated(false);
    setFailed(false);
    setOpen(null);
    void (async () => {
      try {
        const listing = await api.sparql(subjectsQuery(source.iri));
        const types = await api.sparql(typesQuery(source.iri));
        if (!live) return;
        setTruncated(listing.truncated);
        setSubjects(subjectsFromSparql(listing.rows, types.rows));
      } catch {
        if (live) setFailed(true);
      }
    })();
    return () => {
      live = false;
    };
  }, [source.iri]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card size="small" style={{ background: colors.fill, border: "none" }}>
        <Space
          style={{ width: "100%", justifyContent: "space-between" }}
          align="center"
          wrap
        >
          <Space size={8} align="center" wrap>
            <Title level={5} style={{ margin: 0, fontWeight: 600 }}>
              {source.name}
            </Title>
            <Tag>{packLabel}</Tag>
            {subjects !== null && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {COPY.triplesOf(source.triples)}
              </Text>
            )}
          </Space>
          <Button size="small" onClick={onReconcile}>
            {COPY.reconcile}
          </Button>
        </Space>
      </Card>

      <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }}>
        {COPY.hint}
      </Paragraph>

      {failed && <Alert type="error" showIcon message={COPY.failed} />}
      {truncated && <Alert type="warning" showIcon message={COPY.truncated} />}

      {open ? (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Button size="small" onClick={() => setOpen(null)}>
            {COPY.back}
          </Button>
          <SubjectExplorer seed={open.iri} label={open.localName} colors={colors} />
        </Space>
      ) : (
        <Card size="small">
          <Table
            size="small"
            rowKey="iri"
            loading={subjects === null}
            dataSource={[...(subjects ?? [])]}
            pagination={{ pageSize: 15, size: "small" }}
            columns={[
              {
                title: COPY.name,
                dataIndex: "localName",
                key: "name",
                width: 260,
                render: (name: string) => <Text style={{ fontWeight: 500 }}>{name}</Text>,
              },
              {
                title: COPY.kind,
                dataIndex: "kind",
                key: "kind",
                width: 200,
                render: (kind: string | null) =>
                  kind ? <Tag>{kind}</Tag> : <Text type="secondary">{COPY.emptyKind}</Text>,
              },
              {
                title: COPY.triples,
                dataIndex: "triples",
                key: "triples",
                width: 90,
                align: "right",
              },
            ]}
            onRow={(row) => ({
              onClick: () => setOpen(row),
              style: { cursor: "pointer" },
            })}
            locale={{ emptyText: COPY.none }}
          />
        </Card>
      )}
    </Space>
  );
}
