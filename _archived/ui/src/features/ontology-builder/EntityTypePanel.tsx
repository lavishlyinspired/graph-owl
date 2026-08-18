/** Detail panel for a selected entity type: edit metadata, manage
 *  attributes, and delete the type. */

import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../../components/ui/card";
import { ConfirmButton } from "../../components/ui/confirm-button";
import { Input } from "../../components/ui/input";
import { Label } from "../../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../components/ui/select";
import { Switch } from "../../components/ui/switch";
import { Textarea } from "../../components/ui/textarea";
import type { Attribute, DataType, EntityType, OntologyModel } from "./types";
import {
  DATA_TYPE_LABELS,
  addAttribute,
  removeAttribute,
  removeEntityType,
  updateAttribute,
  updateEntityType,
} from "./state";

interface EntityTypePanelProps {
  readonly model: OntologyModel;
  readonly entity: EntityType;
  readonly onChange: (model: OntologyModel) => void;
}

const COPY = {
  entityTypeLabel: "Entity type",
  name: "Name",
  displayName: "Display name",
  description: "Description",
  colour: "Colour",
  save: "Save",
  cancel: "Cancel",
  edit: "Edit",
  delete: "Delete",
  deleteEntityTitle: "Delete this entity type?",
  deleteEntityDescription: "Its relationships will also be removed.",
  relationshipsLabel: "Relationships",
  attributesLabel: "Attributes",
  tableName: "Name",
  tableType: "Type",
  tableRequired: "Required",
  tableActions: "Actions",
  noAttributes: "No attributes yet.",
  editAttribute: "Edit attribute",
  addAttribute: "Add attribute",
  dataType: "Data type",
  references: "References",
  selectEntityType: "Select an entity type",
  required: "Required",
  update: "Update",
  add: "Add",
  yes: "Yes",
  no: "No",
};

const EMPTY_ATTR = {
  name: "",
  displayName: "",
  description: "",
  dataType: "string" as DataType,
  required: false,
  referenceToId: undefined as string | undefined,
};

export function EntityTypePanel({ model, entity, onChange }: EntityTypePanelProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({
    name: entity.name,
    displayName: entity.displayName,
    description: entity.description,
    color: entity.color,
  });

  const [attrDraft, setAttrDraft] = useState(EMPTY_ATTR);
  const [editingAttrId, setEditingAttrId] = useState<string | null>(null);

  const relationshipCount = model.relationships.filter(
    (r) => r.fromEntityTypeId === entity.id || r.toEntityTypeId === entity.id,
  ).length;

  const startEdit = () => {
    setDraft({
      name: entity.name,
      displayName: entity.displayName,
      description: entity.description,
      color: entity.color,
    });
    setEditing(true);
  };

  const save = () => {
    onChange(updateEntityType(model, entity.id, draft));
    setEditing(false);
  };

  const saveAttribute = () => {
    if (!attrDraft.name.trim()) return;
    const payload = {
      ...attrDraft,
      referenceToId: attrDraft.dataType === "reference" ? attrDraft.referenceToId ?? null : null,
    };
    if (editingAttrId) {
      onChange(updateAttribute(model, entity.id, editingAttrId, payload));
    } else {
      onChange(addAttribute(model, entity.id, payload));
    }
    setEditingAttrId(null);
    setAttrDraft(EMPTY_ATTR);
  };

  const startEditAttribute = (attr: Attribute) => {
    setAttrDraft({
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
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-xs text-[var(--gowl-text-subtle)]">{COPY.entityTypeLabel}</p>
        <h4 className="m-0 text-lg font-semibold text-[var(--gowl-text)]">
          {entity.displayName || entity.name}
        </h4>
      </div>

      {editing ? (
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.name}</Label>
            <Input value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.displayName}</Label>
            <Input
              value={draft.displayName}
              onChange={(e) => setDraft((d) => ({ ...d, displayName: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.description}</Label>
            <Textarea
              rows={3}
              value={draft.description}
              onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.colour}</Label>
            <div className="flex items-center gap-2">
              <input
                type="color"
                value={draft.color}
                onChange={(e) => setDraft((d) => ({ ...d, color: e.target.value }))}
                className="h-9 w-9 cursor-pointer rounded-[var(--gowl-radius-small)] border border-[var(--gowl-border)] bg-transparent p-0.5"
              />
              <span className="font-mono text-xs text-[var(--gowl-text-subtle)]">{draft.color}</span>
            </div>
          </div>
          <div className="flex gap-2">
            <Button onClick={save}>{COPY.save}</Button>
            <Button variant="outline" onClick={() => setEditing(false)}>
              {COPY.cancel}
            </Button>
          </div>
        </div>
      ) : (
        <Card>
          <CardContent className="flex flex-col gap-2 pt-4 text-sm">
            <DescriptionRow label={COPY.name} value={entity.name} />
            <DescriptionRow label={COPY.description} value={entity.description || "—"} />
            <DescriptionRow label={COPY.relationshipsLabel} value={String(relationshipCount)} />
            <DescriptionRow label={COPY.attributesLabel} value={String(entity.attributes.length)} />
            <div className="mt-2 flex gap-2">
              <Button variant="outline" size="sm" onClick={startEdit}>
                {COPY.edit}
              </Button>
              <ConfirmButton
                variant="destructive"
                size="sm"
                title={COPY.deleteEntityTitle}
                description={COPY.deleteEntityDescription}
                onConfirm={() => onChange(removeEntityType(model, entity.id))}
              >
                {COPY.delete}
              </ConfirmButton>
            </div>
          </CardContent>
        </Card>
      )}

      <div>
        <h5 className="mb-2 text-sm font-semibold text-[var(--gowl-text)]">{COPY.attributesLabel}</h5>
        <div className="overflow-hidden rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)]">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-[var(--gowl-surface)] text-left text-xs text-[var(--gowl-text-muted)]">
                <th className="px-3 py-2 font-medium">{COPY.tableName}</th>
                <th className="px-3 py-2 font-medium">{COPY.tableType}</th>
                <th className="px-3 py-2 font-medium">{COPY.tableRequired}</th>
                <th className="px-3 py-2 font-medium">{COPY.tableActions}</th>
              </tr>
            </thead>
            <tbody>
              {entity.attributes.map((attr) => (
                <tr key={attr.id} className="border-t border-[var(--gowl-border-soft)]">
                  <td className="px-3 py-2">{attr.name}</td>
                  <td className="px-3 py-2">{DATA_TYPE_LABELS[attr.dataType]}</td>
                  <td className="px-3 py-2">{attr.required ? COPY.yes : COPY.no}</td>
                  <td className="px-3 py-2">
                    <div className="flex gap-2">
                      <Button size="sm" variant="outline" onClick={() => startEditAttribute(attr)}>
                        {COPY.edit}
                      </Button>
                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() => onChange(removeAttribute(model, entity.id, attr.id))}
                      >
                        {COPY.delete}
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
              {entity.attributes.length === 0 && (
                <tr>
                  <td className="px-3 py-4 text-center text-[var(--gowl-text-subtle)]" colSpan={4}>
                    {COPY.noAttributes}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{editingAttrId ? COPY.editAttribute : COPY.addAttribute}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.name}</Label>
            <Input value={attrDraft.name} onChange={(e) => setAttrDraft((a) => ({ ...a, name: e.target.value }))} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.displayName}</Label>
            <Input
              value={attrDraft.displayName}
              onChange={(e) => setAttrDraft((a) => ({ ...a, displayName: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.description}</Label>
            <Textarea
              rows={2}
              value={attrDraft.description}
              onChange={(e) => setAttrDraft((a) => ({ ...a, description: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.dataType}</Label>
            <Select
              value={attrDraft.dataType}
              onValueChange={(dataType) => setAttrDraft((a) => ({ ...a, dataType: dataType as DataType }))}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(DATA_TYPE_LABELS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {attrDraft.dataType === "reference" && (
            <div className="flex flex-col gap-1.5">
              <Label>{COPY.references}</Label>
              <Select
                value={attrDraft.referenceToId ?? ""}
                onValueChange={(referenceToId) =>
                  setAttrDraft((a) => ({ ...a, referenceToId: referenceToId as string | undefined }))
                }
              >
                <SelectTrigger>
                  <SelectValue placeholder={COPY.selectEntityType} />
                </SelectTrigger>
                <SelectContent>
                  {model.entityTypes
                    .filter((et) => et.id !== entity.id)
                    .map((et) => (
                      <SelectItem key={et.id} value={et.id}>
                        {et.displayName || et.name}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="flex items-center gap-2">
            <Switch
              checked={attrDraft.required}
              onCheckedChange={(required) => setAttrDraft((a) => ({ ...a, required }))}
            />
            <Label>{COPY.required}</Label>
          </div>
          <div className="flex gap-2">
            <Button onClick={saveAttribute} disabled={!attrDraft.name.trim()}>
              {editingAttrId ? COPY.update : COPY.add}
            </Button>
            {editingAttrId && (
              <Button
                variant="outline"
                onClick={() => {
                  setEditingAttrId(null);
                  setAttrDraft(EMPTY_ATTR);
                }}
              >
                {COPY.cancel}
              </Button>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function DescriptionRow({ label, value }: { readonly label: string; readonly value: string }) {
  return (
    <div className="flex justify-between gap-4">
      <span className="text-[var(--gowl-text-subtle)]">{label}</span>
      <span className="text-right text-[var(--gowl-text)]">{value}</span>
    </div>
  );
}
