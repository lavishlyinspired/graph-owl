/** Detail panel for a selected relationship. Allows editing name,
 *  cardinality, endpoints, and deleting the edge. */

import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Card, CardContent } from "../../components/ui/card";
import { ConfirmButton } from "../../components/ui/confirm-button";
import { Input } from "../../components/ui/input";
import { Label } from "../../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../components/ui/select";
import { Textarea } from "../../components/ui/textarea";
import type { Cardinality, OntologyModel, Relationship } from "./types";
import { CARDINALITY_LABELS, removeRelationship, updateRelationship } from "./state";

const COPY = {
  relationshipLabel: "Relationship",
  name: "Name",
  displayName: "Display name",
  description: "Description",
  from: "From",
  to: "To",
  cardinality: "Cardinality",
  save: "Save",
  cancel: "Cancel",
  edit: "Edit",
  delete: "Delete",
  deleteTitle: "Delete this relationship?",
};

interface RelationshipPanelProps {
  readonly model: OntologyModel;
  readonly relationship: Relationship;
  readonly onChange: (model: OntologyModel) => void;
}

export function RelationshipPanel({ model, relationship, onChange }: RelationshipPanelProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({
    name: relationship.name,
    displayName: relationship.displayName,
    description: relationship.description,
    fromEntityTypeId: relationship.fromEntityTypeId,
    toEntityTypeId: relationship.toEntityTypeId,
    cardinality: relationship.cardinality,
  });

  const fromName =
    model.entityTypes.find((e) => e.id === relationship.fromEntityTypeId)?.displayName ?? "—";
  const toName =
    model.entityTypes.find((e) => e.id === relationship.toEntityTypeId)?.displayName ?? "—";

  const startEdit = () => {
    setDraft({
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
    onChange(updateRelationship(model, relationship.id, draft));
    setEditing(false);
  };

  const entityOptions = model.entityTypes.map((et) => ({
    value: et.id,
    label: et.displayName || et.name,
  }));

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-xs text-[var(--gowl-text-subtle)]">{COPY.relationshipLabel}</p>
        <h4 className="m-0 text-lg font-semibold text-[var(--gowl-text)]">
          {relationship.displayName || relationship.name}
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
            <Label>{COPY.from}</Label>
            <Select
              value={draft.fromEntityTypeId}
              onValueChange={(fromEntityTypeId) =>
                setDraft((d) => ({ ...d, fromEntityTypeId: fromEntityTypeId as string }))
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {entityOptions.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.to}</Label>
            <Select
              value={draft.toEntityTypeId}
              onValueChange={(toEntityTypeId) =>
                setDraft((d) => ({ ...d, toEntityTypeId: toEntityTypeId as string }))
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {entityOptions.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.cardinality}</Label>
            <Select
              value={draft.cardinality}
              onValueChange={(cardinality) => setDraft((d) => ({ ...d, cardinality: cardinality as Cardinality }))}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(CARDINALITY_LABELS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
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
            <DescriptionRow label={COPY.name} value={relationship.name} />
            <DescriptionRow label={COPY.description} value={relationship.description || "—"} />
            <DescriptionRow label={COPY.from} value={fromName} />
            <DescriptionRow label={COPY.to} value={toName} />
            <DescriptionRow label={COPY.cardinality} value={CARDINALITY_LABELS[relationship.cardinality]} />
            <div className="mt-2 flex gap-2">
              <Button variant="outline" size="sm" onClick={startEdit}>
                {COPY.edit}
              </Button>
              <ConfirmButton
                variant="destructive"
                size="sm"
                title={COPY.deleteTitle}
                onConfirm={() => onChange(removeRelationship(model, relationship.id))}
              >
                {COPY.delete}
              </ConfirmButton>
            </div>
          </CardContent>
        </Card>
      )}
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
