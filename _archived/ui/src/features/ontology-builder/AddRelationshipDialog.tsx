/** Dialog for adding a new relationship between two existing entity types. */

import { useState } from "react";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "../../components/ui/dialog";
import { Input } from "../../components/ui/input";
import { Label } from "../../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../components/ui/select";
import { Textarea } from "../../components/ui/textarea";
import type { Cardinality, OntologyModel } from "./types";
import { addRelationship, CARDINALITY_LABELS } from "./state";

const COPY = {
  trigger: "Add relationship",
  dialogTitle: "Add relationship",
  name: "Name",
  displayName: "Display name",
  description: "Description",
  from: "From",
  to: "To",
  cardinality: "Cardinality",
  selectEntityType: "Select an entity type",
  selectCardinality: "Select cardinality",
  cancel: "Cancel",
};

interface AddRelationshipDialogProps {
  readonly model: OntologyModel;
  readonly onChange: (model: OntologyModel) => void;
}

const EMPTY = { name: "", displayName: "", description: "", from: "", to: "", cardinality: "" as Cardinality | "" };

export function AddRelationshipDialog({ model, onChange }: AddRelationshipDialogProps) {
  const [open, setOpen] = useState(false);
  const [values, setValues] = useState(EMPTY);

  const entityOptions = model.entityTypes.map((et) => ({
    value: et.id,
    label: et.displayName || et.name,
  }));

  const canSubmit =
    values.name.trim().length > 0 && values.from !== "" && values.to !== "" && values.cardinality !== "";

  const submit = () => {
    if (!canSubmit) return;
    onChange(
      addRelationship(model, {
        name: values.name,
        displayName: values.displayName,
        description: values.description,
        fromEntityTypeId: values.from,
        toEntityTypeId: values.to,
        cardinality: values.cardinality as Cardinality,
      }),
    );
    setOpen(false);
    setValues(EMPTY);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setValues(EMPTY);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{COPY.dialogTitle}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="rel-name">{COPY.name}</Label>
            <Input
              id="rel-name"
              value={values.name}
              onChange={(e) => setValues((v) => ({ ...v, name: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="rel-display-name">{COPY.displayName}</Label>
            <Input
              id="rel-display-name"
              value={values.displayName}
              onChange={(e) => setValues((v) => ({ ...v, displayName: e.target.value }))}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="rel-description">{COPY.description}</Label>
            <Textarea
              id="rel-description"
              rows={2}
              value={values.description}
              onChange={(e) => setValues((v) => ({ ...v, description: e.target.value }))}
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{COPY.from}</Label>
              <Select
                value={values.from}
                onValueChange={(from) => setValues((v) => ({ ...v, from: from as string }))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={COPY.selectEntityType} />
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
                value={values.to}
                onValueChange={(to) => setValues((v) => ({ ...v, to: to as string }))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={COPY.selectEntityType} />
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
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{COPY.cardinality}</Label>
            <Select
              value={values.cardinality}
              onValueChange={(cardinality) => setValues((v) => ({ ...v, cardinality: cardinality as Cardinality }))}
            >
              <SelectTrigger>
                <SelectValue placeholder={COPY.selectCardinality} />
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
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            {COPY.cancel}
          </Button>
          <Button onClick={submit} disabled={!canSubmit}>
            {COPY.dialogTitle}
          </Button>
        </DialogFooter>
      </DialogContent>
      <DialogTrigger asChild>
        <Button className="gap-1.5" disabled={model.entityTypes.length < 2}>
          <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M12 5v14M5 12h14" />
          </svg>
          {COPY.trigger}
        </Button>
      </DialogTrigger>
    </Dialog>
  );
}
