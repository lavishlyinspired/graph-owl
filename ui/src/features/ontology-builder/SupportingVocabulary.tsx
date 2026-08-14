/** Manage interactions, reference data, and sources — the counts that sit
 *  beside Entities and Relationships in the ontology header. */

import { useState } from "react";
import {
  Button,
  Card,
  Form,
  Input,
  List,
  Popconfirm,
  Select,
  Space,
  Tabs,
  Typography,
} from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import type { OntologyModel, OntologyPackOption } from "./types";
import {
  addInteraction,
  addReferenceDatum,
  removeInteraction,
  removeReferenceDatum,
  setSources,
} from "./state";

const { Text } = Typography;

interface SupportingVocabularyProps {
  readonly model: OntologyModel;
  readonly packs: readonly OntologyPackOption[];
  readonly onChange: (model: OntologyModel) => void;
}

export function SupportingVocabulary({ model, packs, onChange }: SupportingVocabularyProps) {
  const [interactionForm] = Form.useForm<{ name: string; displayName: string; description: string }>();
  const [referenceForm] = Form.useForm<{ name: string; displayName: string; description: string }>();

  const addInteractionClicked = () => {
    const values = interactionForm.getFieldsValue();
    onChange(addInteraction(model, values));
    interactionForm.resetFields();
  };

  const addReferenceClicked = () => {
    const values = referenceForm.getFieldsValue();
    onChange(addReferenceDatum(model, values));
    referenceForm.resetFields();
  };

  const sourceOptions = packs.map((pack) => ({ value: pack.id, label: pack.name }));

  return (
    <Card size="small" title="Supporting vocabulary">
      <Tabs
        items={[
          {
            key: "interactions",
            label: `Interactions (${model.interactions.length})`,
            children: (
              <Space direction="vertical" style={{ width: "100%" }}>
                <Form form={interactionForm} layout="inline">
                  <Form.Item name="name" rules={[{ required: true }]}>
                    <Input placeholder="Name" />
                  </Form.Item>
                  <Form.Item name="displayName">
                    <Input placeholder="Display name" />
                  </Form.Item>
                  <Form.Item name="description">
                    <Input placeholder="Description" />
                  </Form.Item>
                  <Button icon={<PlusOutlined />} onClick={addInteractionClicked}>
                    Add
                  </Button>
                </Form>
                <List
                  size="small"
                  dataSource={[...model.interactions]}
                  renderItem={(item) => (
                    <List.Item
                      actions={[
                        <Popconfirm
                          key="delete"
                          title="Delete interaction?"
                          onConfirm={() => onChange(removeInteraction(model, item.id))}
                        >
                          <Button size="small" danger icon={<DeleteOutlined />} />
                        </Popconfirm>,
                      ]}
                    >
                      <List.Item.Meta
                        title={item.displayName || item.name}
                        description={item.description || undefined}
                      />
                    </List.Item>
                  )}
                />
              </Space>
            ),
          },
          {
            key: "reference",
            label: `Reference data (${model.referenceData.length})`,
            children: (
              <Space direction="vertical" style={{ width: "100%" }}>
                <Form form={referenceForm} layout="inline">
                  <Form.Item name="name" rules={[{ required: true }]}>
                    <Input placeholder="Name" />
                  </Form.Item>
                  <Form.Item name="displayName">
                    <Input placeholder="Display name" />
                  </Form.Item>
                  <Form.Item name="description">
                    <Input placeholder="Description" />
                  </Form.Item>
                  <Button icon={<PlusOutlined />} onClick={addReferenceClicked}>
                    Add
                  </Button>
                </Form>
                <List
                  size="small"
                  dataSource={[...model.referenceData]}
                  renderItem={(item) => (
                    <List.Item
                      actions={[
                        <Popconfirm
                          key="delete"
                          title="Delete reference datum?"
                          onConfirm={() => onChange(removeReferenceDatum(model, item.id))}
                        >
                          <Button size="small" danger icon={<DeleteOutlined />} />
                        </Popconfirm>,
                      ]}
                    >
                      <List.Item.Meta
                        title={item.displayName || item.name}
                        description={item.description || undefined}
                      />
                    </List.Item>
                  )}
                />
              </Space>
            ),
          },
          {
            key: "sources",
            label: `Sources (${model.sources.length})`,
            children: (
              <Space direction="vertical" style={{ width: "100%" }}>
                <Text type="secondary">
                  Sources are loaded from installed ontology packs. You can also
                  type a source name and add it manually.
                </Text>
                <Select
                  mode="tags"
                  style={{ width: "100%" }}
                  placeholder="Select or type sources"
                  value={model.sources.map((s) => s.name)}
                  options={sourceOptions}
                  onChange={(values) => {
                    const sources = values.map((name) => {
                      const existing = model.sources.find((s) => s.name === name);
                      if (existing) return existing;
                      const pack = packs.find((p) => p.id === name);
                      return {
                        id: crypto.randomUUID(),
                        name,
                        displayName: pack?.name ?? name,
                      };
                    });
                    onChange(setSources(model, sources));
                  }}
                />
              </Space>
            ),
          },
        ]}
      />
    </Card>
  );
}
