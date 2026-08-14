/** Detail panel for a selected relationship. Allows editing name,
 *  cardinality, endpoints, and deleting the edge. */

import { useState } from "react";
import { Button, Card, Descriptions, Form, Input, Popconfirm, Select, Space, Typography } from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import type { OntologyModel, Relationship } from "./types";
import { CARDINALITY_LABELS, removeRelationship, updateRelationship } from "./state";

const { Text } = Typography;

interface RelationshipPanelProps {
  readonly model: OntologyModel;
  readonly relationship: Relationship;
  readonly onChange: (model: OntologyModel) => void;
}

export function RelationshipPanel({ model, relationship, onChange }: RelationshipPanelProps) {
  const [editing, setEditing] = useState(false);
  const [form] = Form.useForm<{
    name: string;
    displayName: string;
    description: string;
    fromEntityTypeId: string;
    toEntityTypeId: string;
    cardinality: keyof typeof CARDINALITY_LABELS;
  }>();

  const fromName =
    model.entityTypes.find((e) => e.id === relationship.fromEntityTypeId)?.displayName ?? "—";
  const toName =
    model.entityTypes.find((e) => e.id === relationship.toEntityTypeId)?.displayName ?? "—";

  const startEdit = () => {
    form.setFieldsValue({
      name: relationship.name,
      displayName: relationship.displayName,
      description: relationship.description,
      fromEntityTypeId: relationship.fromEntityTypeId,
      toEntityTypeId: relationship.toEntityTypeId,
      cardinality: relationship.cardinality,
    });
    setEditing(true);
  };

  const save = () => {
    const values = form.getFieldsValue();
    onChange(updateRelationship(model, relationship.id, values));
    setEditing(false);
  };

  const entityOptions = model.entityTypes.map((et) => ({
    value: et.id,
    label: et.displayName || et.name,
  }));

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Text type="secondary">Relationship</Text>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {relationship.displayName || relationship.name}
        </Typography.Title>
      </div>

      {editing ? (
        <Form form={form} layout="vertical">
          <Form.Item label="Name" name="name" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item label="Display name" name="displayName">
            <Input />
          </Form.Item>
          <Form.Item label="Description" name="description">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item label="From" name="fromEntityTypeId" rules={[{ required: true }]}>
            <Select options={entityOptions} />
          </Form.Item>
          <Form.Item label="To" name="toEntityTypeId" rules={[{ required: true }]}>
            <Select options={entityOptions} />
          </Form.Item>
          <Form.Item label="Cardinality" name="cardinality" rules={[{ required: true }]}>
            <Select
              options={Object.entries(CARDINALITY_LABELS).map(([value, label]) => ({
                value,
                label,
              }))}
            />
          </Form.Item>
          <Space>
            <Button icon={<SaveOutlined />} type="primary" onClick={save}>
              Save
            </Button>
            <Button onClick={() => setEditing(false)}>Cancel</Button>
          </Space>
        </Form>
      ) : (
        <Card size="small">
          <Descriptions column={1} size="small">
            <Descriptions.Item label="Name">{relationship.name}</Descriptions.Item>
            <Descriptions.Item label="Description">
              {relationship.description || "—"}
            </Descriptions.Item>
            <Descriptions.Item label="From">{fromName}</Descriptions.Item>
            <Descriptions.Item label="To">{toName}</Descriptions.Item>
            <Descriptions.Item label="Cardinality">
              {CARDINALITY_LABELS[relationship.cardinality]}
            </Descriptions.Item>
          </Descriptions>
          <Space style={{ marginTop: 12 }}>
            <Button onClick={startEdit}>Edit</Button>
            <Popconfirm
              title="Delete this relationship?"
              onConfirm={() => onChange(removeRelationship(model, relationship.id))}
            >
              <Button danger icon={<DeleteOutlined />}>
                Delete
              </Button>
            </Popconfirm>
          </Space>
        </Card>
      )}
    </Space>
  );
}
