/** Dialog for adding a new entity type to the ontology. */

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
import { Textarea } from "../../components/ui/textarea";
import type { OntologyModel } from "./types";
import { addEntityType, nextEntityColor } from "./state";

const COPY = {
  trigger: "Add entity type",
  dialogTitle: "Add entity type",
  name: "Name",
  displayName: "Display name",
  description: "Description",
  colour: "Colour",
  cancel: "Cancel",
};

interface AddEntityTypeDialogProps {
  readonly model: OntologyModel;
  readonly onChange: (model: OntologyModel) => void;
}

export function AddEntityTypeDialog({ model, onChange }: AddEntityTypeDialogProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [description, setDescription] = useState("");
  const [color, setColor] = useState(() => nextEntityColor(model.entityTypes.length));

  const reset = () => {
    setName("");
    setDisplayName("");
    setDescription("");
    setColor(nextEntityColor(model.entityTypes.length));
  };

  const submit = () => {
    if (!name.trim()) return;
    onChange(addEntityType(model, { name, displayName, description, color }));
    setOpen(false);
    reset();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) reset();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{COPY.dialogTitle}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="entity-type-name">{COPY.name}</Label>
            <Input id="entity-type-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="entity-type-display-name">{COPY.displayName}</Label>
            <Input
              id="entity-type-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="entity-type-description">{COPY.description}</Label>
            <Textarea
              id="entity-type-description"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="entity-type-color">{COPY.colour}</Label>
            <div className="flex items-center gap-2">
              <input
                id="entity-type-color"
                type="color"
                value={color}
                onChange={(e) => setColor(e.target.value)}
                className="h-9 w-9 cursor-pointer rounded-[var(--gowl-radius-small)] border border-[var(--gowl-border)] bg-transparent p-0.5"
              />
              <span className="font-mono text-xs text-[var(--gowl-text-subtle)]">{color}</span>
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            {COPY.cancel}
          </Button>
          <Button onClick={submit} disabled={!name.trim()}>
            {COPY.dialogTitle}
          </Button>
        </DialogFooter>
      </DialogContent>
      <DialogTrigger asChild>
        <Button className="gap-1.5">
          <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M12 5v14M5 12h14" />
          </svg>
          {COPY.trigger}
        </Button>
      </DialogTrigger>
    </Dialog>
  );
}
