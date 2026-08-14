/** Visual ontology builder prototype.
 *
 *  A standalone client surface for designing entity types, their attributes,
 *  and the relationships between them. The model persists to `localStorage`
 *  and can be exported/imported as JSON; there is no server-side
 *  `/entity-types` collection yet, so the pack selector wires to the existing
 *  `GET /ontology-packs` endpoint for read-only source names.
 *
 *  The layout follows the reference screenshots: a top bar with pack selector
 *  and counts, a diagram canvas with layout/edge controls, a filter/search
 *  strip, zoom controls, and a right-hand detail panel. */

import { useEffect, useMemo, useState } from "react";
import {
  Badge,
  Button,
  Card,
  Drawer,
  Empty,
  Flex,
  Segmented,
  Select,
  Space,
  Tooltip,
  Typography,
  Upload,
  message,
} from "antd";
import FilterOutlined from "@ant-design/icons/es/icons/FilterOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import DownloadOutlined from "@ant-design/icons/es/icons/DownloadOutlined";
import UploadOutlined from "@ant-design/icons/es/icons/UploadOutlined";
import { palette } from "../../theme";
import { api } from "../../api";
import { AddEntityTypeDialog } from "./AddEntityTypeDialog";
import { AddRelationshipDialog } from "./AddRelationshipDialog";
import { EntityTypePanel } from "./EntityTypePanel";
import { RelationshipPanel } from "./RelationshipPanel";
import { SupportingVocabulary } from "./SupportingVocabulary";
import { OntologyCanvas, type EdgeStyle, type LayoutName } from "./OntologyCanvas";
import { exportJson, importJson, loadModel, saveModel } from "./state";
import { entityById, relationshipById, toCytoscapeElements } from "./cytoscapeModel";
import type { OntologyModel, OntologyPackOption } from "./types";

const { Title, Text } = Typography;

type Colors = (typeof palette)["light"];

const COPY = {
  title: "Ontology Builder",
  subtitle: "Design entity types, attributes, and relationships.",
  packPlaceholder: "Select a pack",
  entities: "Entities",
  relationships: "Relationships",
  interactions: "Interactions",
  referenceData: "Reference Data",
  sources: "Sources",
  layout: "Layout",
  edges: "Edges",
  filter: "Filter",
  resetView: "Reset view",
  export: "Export JSON",
  import: "Import JSON",
  importError: "Could not import ontology.",
  noSelection: "Select a node or edge to edit.",
  emptyState: "No entity types yet. Add one to start building the ontology.",
};

interface OntologyBuilderProps {
  readonly colors: Colors;
}

export function OntologyBuilder({ colors }: OntologyBuilderProps) {
  const [model, setModel] = useState<OntologyModel>(() => loadModel());
  const [layout, setLayout] = useState<LayoutName>("radial");
  const [edgeStyle, setEdgeStyle] = useState<EdgeStyle>("polyline");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [filterText] = useState("");
  const [packs, setPacks] = useState<readonly OntologyPackOption[]>([]);
  const [packsLoading, setPacksLoading] = useState(true);
  const [vocabularyOpen, setVocabularyOpen] = useState(false);

  useEffect(() => {
    saveModel(model);
  }, [model]);

  useEffect(() => {
    let live = true;
    api
      .ontologyPacks()
      .then((list) => {
        if (!live) return;
        setPacks(
          list.map((pack) => ({
            id: pack.id,
            name: pack.packId,
            description: pack.sourceUrl,
          })),
        );
      })
      .catch(() => {
        // Packs are optional — the builder works without them.
      })
      .finally(() => setPacksLoading(false));
    return () => {
      live = false;
    };
  }, []);

  const filteredModel = useMemo(() => {
    const text = filterText.trim().toLowerCase();
    if (!text) return model;
    const entityIds = new Set(
      model.entityTypes
        .filter(
          (et) =>
            et.name.toLowerCase().includes(text) ||
            et.displayName.toLowerCase().includes(text),
        )
        .map((et) => et.id),
    );
    const relationships = model.relationships.filter(
      (r) => entityIds.has(r.fromEntityTypeId) && entityIds.has(r.toEntityTypeId),
    );
    return {
      ...model,
      entityTypes: model.entityTypes.filter((et) => entityIds.has(et.id)),
      relationships,
    };
  }, [model, filterText]);

  const elements = useMemo(
    () => toCytoscapeElements(filteredModel),
    [filteredModel],
  );

  const selectedEntity = useMemo(
    () => (selectedId ? entityById(model, selectedId) ?? null : null),
    [model, selectedId],
  );

  const selectedRelationship = useMemo(
    () => (selectedId ? relationshipById(model, selectedId) ?? null : null),
    [model, selectedId],
  );

  const handleImport = async (file: File) => {
    try {
      const text = await file.text();
      const imported = importJson(text);
      setModel(imported);
      setSelectedId(null);
      message.success("Ontology imported");
    } catch {
      message.error(COPY.importError);
    }
    return false;
  };

  const handleExport = () => {
    const blob = new Blob([exportJson(model)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ontology.json";
    a.click();
    URL.revokeObjectURL(url);
  };

  const counts = (
    <Space size="large" wrap>
      <Badge count={model.entityTypes.length} showZero color={colors.primary}>
        <Text strong>{COPY.entities}</Text>
      </Badge>
      <Badge count={model.relationships.length} showZero color={colors.primary}>
        <Text strong>{COPY.relationships}</Text>
      </Badge>
      <Badge count={model.interactions.length} showZero color={colors.textSubtle}>
        <Text strong>{COPY.interactions}</Text>
      </Badge>
      <Badge count={model.referenceData.length} showZero color={colors.textSubtle}>
        <Text strong>{COPY.referenceData}</Text>
      </Badge>
      <Badge count={model.sources.length} showZero color={colors.textSubtle}>
        <Text strong>{COPY.sources}</Text>
      </Badge>
    </Space>
  );

  return (
    <Flex vertical style={{ height: "100%" }}>
      {/* Header */}
      <Flex
        justify="space-between"
        align="center"
        style={{
          padding: "12px 16px",
          borderBottom: `1px solid ${colors.border}`,
          background: colors.raised,
        }}
      >
        <Space size="large">
          <div>
            <Title level={4} style={{ margin: 0 }}>
              {COPY.title}
            </Title>
            <Text type="secondary">{COPY.subtitle}</Text>
          </div>
          <Select
            loading={packsLoading}
            placeholder={COPY.packPlaceholder}
            style={{ minWidth: 200 }}
            options={packs.map((pack) => ({
              value: pack.id,
              label: pack.name,
            }))}
            allowClear
          />
        </Space>
        {counts}
      </Flex>

      {/* Toolbar */}
      <Flex
        justify="space-between"
        align="center"
        wrap
        style={{
          padding: "10px 16px",
          borderBottom: `1px solid ${colors.border}`,
          background: colors.raised,
          gap: 12,
        }}
      >
        <Space wrap>
          <AddEntityTypeDialog model={model} onChange={setModel} />
          <AddRelationshipDialog model={model} onChange={setModel} />
          <Button icon={<FilterOutlined />} onClick={() => setVocabularyOpen(true)}>
            {COPY.filter}
          </Button>
        </Space>
        <Space wrap>
          <Text type="secondary">{COPY.layout}</Text>
          <Segmented<LayoutName>
            value={layout}
            onChange={setLayout}
            options={[
              { value: "radial", label: "Radial" },
              { value: "tree", label: "Tree" },
              { value: "force", label: "Force" },
            ]}
          />
          <Text type="secondary">{COPY.edges}</Text>
          <Segmented<EdgeStyle>
            value={edgeStyle}
            onChange={setEdgeStyle}
            options={[
              { value: "polyline", label: "Polyline" },
              { value: "orthogonal", label: "Orthogonal" },
            ]}
          />
          <Tooltip title={COPY.resetView}>
            <Button icon={<ReloadOutlined />} onClick={() => setLayout((l) => l)} />
          </Tooltip>
          <Button icon={<DownloadOutlined />} onClick={handleExport}>
            {COPY.export}
          </Button>
          <Upload beforeUpload={handleImport} showUploadList={false} accept="application/json,.json">
            <Button icon={<UploadOutlined />}>{COPY.import}</Button>
          </Upload>
        </Space>
      </Flex>

      {/* Main canvas + optional side panel */}
      <Flex style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
        <div style={{ flex: 1, padding: 16, minWidth: 0 }}>
          {model.entityTypes.length === 0 ? (
            <Card style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center" }}>
              <Empty description={COPY.emptyState} />
            </Card>
          ) : (
            <OntologyCanvas
              elements={elements}
              colors={colors}
              layout={layout}
              edgeStyle={edgeStyle}
              selectedId={selectedId}
              onSelectNode={setSelectedId}
              onSelectEdge={setSelectedId}
              onClearSelection={() => setSelectedId(null)}
            />
          )}
        </div>

        {selectedEntity && (
          <div
            style={{
              width: 360,
              borderLeft: `1px solid ${colors.border}`,
              padding: 16,
              overflow: "auto",
              background: colors.raised,
            }}
          >
            <EntityTypePanel model={model} entity={selectedEntity} onChange={setModel} />
          </div>
        )}
        {selectedRelationship && !selectedEntity && (
          <div
            style={{
              width: 360,
              borderLeft: `1px solid ${colors.border}`,
              padding: 16,
              overflow: "auto",
              background: colors.raised,
            }}
          >
            <RelationshipPanel model={model} relationship={selectedRelationship} onChange={setModel} />
          </div>
        )}
      </Flex>

      <Drawer
        title="Supporting vocabulary"
        placement="left"
        open={vocabularyOpen}
        onClose={() => setVocabularyOpen(false)}
        width={560}
      >
        <SupportingVocabulary model={model} packs={packs} onChange={setModel} />
      </Drawer>
    </Flex>
  );
}
