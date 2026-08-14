/** Detail panel for a selected entity type: edit metadata, manage
 *  attributes, and delete the type. */

import { useState } from "react";
import {
  Button,
  Card,
  ColorPicker,
  Descriptions,
  Form,
  Input,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Typography,
} from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import type { EntityType, OntologyModel } from "./types";
import { DATA_TYPE_LABELS, removeEntityType, updateEntityType } from "./state";
import { addAttribute, removeAttribute, updateAttribute } from "./state";
import type { Attribute, DataType } from "./types";

const { Text } = Typography;

interface EntityTypePanelProps {
  readonly model: OntologyModel;
  readonly entity: EntityType;
  readonly onChange: (model: OntologyModel) => void;
}

export function EntityTypePanel({ model, entity, onChange }: EntityTypePanelProps) {
  const [editing, setEditing] = useState(false);
  const [form] = Form.useForm<{
    name: string;
    displayName: string;
    description: string;
    color: string;
  }>();

  const [attrForm] = Form.useForm<{
    name: string;
    displayName: string;
    description: string;
    dataType: DataType;
    required: boolean;
    referenceToId: string | undefined;
  }>();

  const [editingAttrId, setEditingAttrId] = useState<string | null>(null);

  const relationshipCount = model.relationships.filter(
    (r) => r.fromEntityTypeId === entity.id || r.toEntityTypeId === entity.id,
  ).length;

  const startEdit = () => {
    form.setFieldsValue({
      name: entity.name,
      displayName: entity.displayName,
      description: entity.description,
      color: entity.color,
    });
    setEditing(true);
  };

  const save = () => {
    const values = form.getFieldsValue();
    onChange(updateEntityType(model, entity.id, values));
    setEditing(false);
  };

  const saveAttribute = () => {
    const values = attrForm.getFieldsValue();
    const payload = {
      ...values,
      referenceToId: values.dataType === "reference" ? values.referenceToId ?? null : null,
    };
    if (editingAttrId) {
      onChange(updateAttribute(model, entity.id, editingAttrId, payload));
    } else {
      onChange(addAttribute(model, entity.id, payload));
    }
    setEditingAttrId(null);
    attrForm.resetFields();
  };

  const startEditAttribute = (attr: Attribute) => {
    attrForm.setFieldsValue({
      name: attr.name,
      displayName: attr.displayName,
      description: attr.description,
      dataType: attr.dataType,
      required: attr.required,
      referenceToId: attr.referenceToId ?? undefined,
    });
    setEditingAttrId(attr.id);
  };

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Text type="secondary">Entity type</Text>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {entity.displayName || entity.name}
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
          <Form.Item label="Colour" name="color">
            <ColorPicker showText />
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
            <Descriptions.Item label="Name">{entity.name}</Descriptions.Item>
            <Descriptions.Item label="Description">
              {entity.description || "—"}
            </Descriptions.Item>
            <Descriptions.Item label="Relationships">{relationshipCount}</Descriptions.Item>
            <Descriptions.Item label="Attributes">{entity.attributes.length}</Descriptions.Item>
          </Descriptions>
          <Space style={{ marginTop: 12 }}>
            <Button onClick={startEdit}>Edit</Button>
            <Popconfirm
              title="Delete this entity type?"
              description="Its relationships will also be removed."
              onConfirm={() => onChange(removeEntityType(model, entity.id))}
            >
              <Button danger icon={<DeleteOutlined />}>
                Delete
              </Button>
            </Popconfirm>
          </Space>
        </Card>
      )}

      <div>
        <Typography.Title level={5}>Attributes</Typography.Title>
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={[...entity.attributes]}
          columns={[
            { title: "Name", dataIndex: "name" },
            { title: "Type", dataIndex: "dataType", render: (t) => DATA_TYPE_LABELS[t as DataType] },
            {
              title: "Required",
              dataIndex: "required",
              render: (required) => (required ? "Yes" : "No"),
            },
            {
              title: "Actions",
              key: "actions",
              render: (_, attr) => (
                <Space>
                  <Button size="small" onClick={() => startEditAttribute(attr)}>
                    Edit
                  </Button>
                  <Button
                    size="small"
                    danger
                    icon={<DeleteOutlined />}
                    onClick={() => onChange(removeAttribute(model, entity.id, attr.id))}
                  >
                    Delete
                  </Button>
                </Space>
              ),
            },
          ]}
        />
      </div>

      <Card size="small" title={editingAttrId ? "Edit attribute" : "Add attribute"}>
        <Form form={attrForm} layout="vertical">
          <Form.Item label="Name" name="name" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item label="Display name" name="displayName">
            <Input />
          </Form.Item>
          <Form.Item label="Description" name="description">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item label="Data type" name="dataType" initialValue="string">
            <Select
              options={Object.entries(DATA_TYPE_LABELS).map(([value, label]) => ({
                value,
                label,
              }))}
            />
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.dataType !== cur.dataType}>
            {({ getFieldValue }) =>
              getFieldValue("dataType") === "reference" ? (
                <Form.Item label="References" name="referenceToId" rules={[{ required: true }]}>
                  <Select
                    options={model.entityTypes
                      .filter((et) => et.id !== entity.id)
                      .map((et) => ({ value: et.id, label: et.displayName || et.name }))}
                    placeholder="Select an entity type"
                  />
                </Form.Item>
              ) : null
            }
          </Form.Item>
          <Form.Item label="Required" name="required" valuePropName="checked" initialValue={false}>
            <Switch />
          </Form.Item>
          <Space>
            <Button type="primary" icon={<SaveOutlined />} onClick={saveAttribute}>
              {editingAttrId ? "Update" : "Add"}
            </Button>
            {editingAttrId && (
              <Button
                onClick={() => {
                  setEditingAttrId(null);
                  attrForm.resetFields();
                }}
              >
                Cancel
              </Button>
            )}
          </Space>
        </Form>
      </Card>
    </Space>
  );
}
