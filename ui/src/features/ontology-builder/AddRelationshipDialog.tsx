/** Dialog for adding a new relationship between two existing entity types. */

import { Button, Form, Input, Modal, Select, Space } from "antd";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import { useState } from "react";
import type { OntologyModel } from "./types";
import { addRelationship, CARDINALITY_LABELS } from "./state";

interface AddRelationshipDialogProps {
  readonly model: OntologyModel;
  readonly onChange: (model: OntologyModel) => void;
}

export function AddRelationshipDialog({ model, onChange }: AddRelationshipDialogProps) {
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<{
    name: string;
    displayName: string;
    description: string;
    fromEntityTypeId: string;
    toEntityTypeId: string;
    cardinality: keyof typeof CARDINALITY_LABELS;
  }>();

  const entityOptions = model.entityTypes.map((et) => ({
    value: et.id,
    label: et.displayName || et.name,
  }));

  const submit = () => {
    const values = form.getFieldsValue();
    onChange(addRelationship(model, values));
    setOpen(false);
    form.resetFields();
  };

  return (
    <>
      <Button
        type="primary"
        icon={<PlusOutlined />}
        onClick={() => setOpen(true)}
        disabled={model.entityTypes.length < 2}
      >
        Add relationship
      </Button>
      <Modal
        title="Add relationship"
        open={open}
        onOk={submit}
        onCancel={() => setOpen(false)}
        destroyOnClose
      >
        <Form form={form} layout="vertical" preserve={false}>
          <Form.Item label="Name" name="name" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item label="Display name" name="displayName">
            <Input />
          </Form.Item>
          <Form.Item label="Description" name="description">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Space style={{ width: "100%" }}>
            <Form.Item label="From" name="fromEntityTypeId" rules={[{ required: true }]}>
              <Select options={entityOptions} style={{ minWidth: 160 }} />
            </Form.Item>
            <Form.Item label="To" name="toEntityTypeId" rules={[{ required: true }]}>
              <Select options={entityOptions} style={{ minWidth: 160 }} />
            </Form.Item>
          </Space>
          <Form.Item label="Cardinality" name="cardinality" rules={[{ required: true }]}>
            <Select
              options={Object.entries(CARDINALITY_LABELS).map(([value, label]) => ({
                value,
                label,
              }))}
            />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}
