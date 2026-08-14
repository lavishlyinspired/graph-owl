/** Dialog for adding a new entity type to the ontology. */

import { Button, ColorPicker, Form, Input, Modal } from "antd";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import { useState } from "react";
import type { OntologyModel } from "./types";
import { addEntityType, nextEntityColor } from "./state";

interface AddEntityTypeDialogProps {
  readonly model: OntologyModel;
  readonly onChange: (model: OntologyModel) => void;
}

export function AddEntityTypeDialog({ model, onChange }: AddEntityTypeDialogProps) {
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<{
    name: string;
    displayName: string;
    description: string;
    color: string;
  }>();

  const submit = () => {
    const values = form.getFieldsValue();
    onChange(addEntityType(model, values));
    setOpen(false);
    form.resetFields();
  };

  return (
    <>
      <Button type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
        Add entity type
      </Button>
      <Modal
        title="Add entity type"
        open={open}
        onOk={submit}
        onCancel={() => setOpen(false)}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          preserve={false}
          initialValues={{ color: nextEntityColor(model.entityTypes.length) }}
        >
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
        </Form>
      </Modal>
    </>
  );
}
