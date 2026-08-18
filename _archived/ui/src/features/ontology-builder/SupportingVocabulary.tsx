/** Manage interactions, reference data, and sources — the counts that sit
 *  beside Entities and Relationships in the ontology header. */

import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../../components/ui/card";
import { ConfirmButton } from "../../components/ui/confirm-button";
import { Input } from "../../components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import type { OntologyModel, OntologyPackOption } from "./types";
import {
  addInteraction,
  addReferenceDatum,
  removeInteraction,
  removeReferenceDatum,
  setSources,
} from "./state";

interface SupportingVocabularyProps {
  readonly model: OntologyModel;
  readonly packs: readonly OntologyPackOption[];
  readonly onChange: (model: OntologyModel) => void;
}

const COPY = {
  title: "Supporting vocabulary",
  interactionsTab: "Interactions",
  referenceTab: "Reference data",
  sourcesTab: "Sources",
  name: "Name",
  displayName: "Display name",
  description: "Description",
  add: "Add",
  delete: "Delete",
  deleteInteractionTitle: "Delete interaction?",
  deleteReferenceTitle: "Delete reference datum?",
  removeSource: (label: string) => `Remove ${label}`,
  sourcesHelp:
    "Sources are loaded from installed ontology packs. You can also type a source name and press Enter to add it manually.",
  selectOrTypeSource: "Select or type a source",
  removeGlyph: "×",
};

const EMPTY_ITEM = { name: "", displayName: "", description: "" };

export function SupportingVocabulary({ model, packs, onChange }: SupportingVocabularyProps) {
  const [interactionDraft, setInteractionDraft] = useState(EMPTY_ITEM);
  const [referenceDraft, setReferenceDraft] = useState(EMPTY_ITEM);
  const [sourceInput, setSourceInput] = useState("");

  const addInteractionClicked = () => {
    if (!interactionDraft.name.trim()) return;
    onChange(addInteraction(model, interactionDraft));
    setInteractionDraft(EMPTY_ITEM);
  };

  const addReferenceClicked = () => {
    if (!referenceDraft.name.trim()) return;
    onChange(addReferenceDatum(model, referenceDraft));
    setReferenceDraft(EMPTY_ITEM);
  };

  const addSource = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed || model.sources.some((s) => s.name === trimmed)) return;
    const pack = packs.find((p) => p.id === trimmed);
    onChange(
      setSources(model, [
        ...model.sources,
        { id: crypto.randomUUID(), name: trimmed, displayName: pack?.name ?? trimmed },
      ]),
    );
    setSourceInput("");
  };

  const removeSource = (id: string) => {
    onChange(setSources(model, model.sources.filter((s) => s.id !== id)));
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>{COPY.title}</CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="interactions">
          <TabsList>
            <TabsTrigger value="interactions">{`${COPY.interactionsTab} (${model.interactions.length})`}</TabsTrigger>
            <TabsTrigger value="reference">{`${COPY.referenceTab} (${model.referenceData.length})`}</TabsTrigger>
            <TabsTrigger value="sources">{`${COPY.sourcesTab} (${model.sources.length})`}</TabsTrigger>
          </TabsList>

          <TabsContent value="interactions">
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <Input
                  placeholder={COPY.name}
                  className="max-w-[160px]"
                  value={interactionDraft.name}
                  onChange={(e) => setInteractionDraft((d) => ({ ...d, name: e.target.value }))}
                />
                <Input
                  placeholder={COPY.displayName}
                  className="max-w-[160px]"
                  value={interactionDraft.displayName}
                  onChange={(e) => setInteractionDraft((d) => ({ ...d, displayName: e.target.value }))}
                />
                <Input
                  placeholder={COPY.description}
                  className="max-w-[200px]"
                  value={interactionDraft.description}
                  onChange={(e) => setInteractionDraft((d) => ({ ...d, description: e.target.value }))}
                />
                <Button onClick={addInteractionClicked} disabled={!interactionDraft.name.trim()}>
                  {COPY.add}
                </Button>
              </div>
              <ul className="flex flex-col gap-1">
                {model.interactions.map((item) => (
                  <li
                    key={item.id}
                    className="flex items-center justify-between rounded-[var(--gowl-radius-small)] border border-[var(--gowl-border-soft)] px-3 py-2"
                  >
                    <div>
                      <div className="text-sm font-medium text-[var(--gowl-text)]">
                        {item.displayName || item.name}
                      </div>
                      {item.description && (
                        <div className="text-xs text-[var(--gowl-text-subtle)]">{item.description}</div>
                      )}
                    </div>
                    <ConfirmButton
                      variant="destructive"
                      size="sm"
                      title={COPY.deleteInteractionTitle}
                      onConfirm={() => onChange(removeInteraction(model, item.id))}
                    >
                      {COPY.delete}
                    </ConfirmButton>
                  </li>
                ))}
              </ul>
            </div>
          </TabsContent>

          <TabsContent value="reference">
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <Input
                  placeholder={COPY.name}
                  className="max-w-[160px]"
                  value={referenceDraft.name}
                  onChange={(e) => setReferenceDraft((d) => ({ ...d, name: e.target.value }))}
                />
                <Input
                  placeholder={COPY.displayName}
                  className="max-w-[160px]"
                  value={referenceDraft.displayName}
                  onChange={(e) => setReferenceDraft((d) => ({ ...d, displayName: e.target.value }))}
                />
                <Input
                  placeholder={COPY.description}
                  className="max-w-[200px]"
                  value={referenceDraft.description}
                  onChange={(e) => setReferenceDraft((d) => ({ ...d, description: e.target.value }))}
                />
                <Button onClick={addReferenceClicked} disabled={!referenceDraft.name.trim()}>
                  {COPY.add}
                </Button>
              </div>
              <ul className="flex flex-col gap-1">
                {model.referenceData.map((item) => (
                  <li
                    key={item.id}
                    className="flex items-center justify-between rounded-[var(--gowl-radius-small)] border border-[var(--gowl-border-soft)] px-3 py-2"
                  >
                    <div>
                      <div className="text-sm font-medium text-[var(--gowl-text)]">
                        {item.displayName || item.name}
                      </div>
                      {item.description && (
                        <div className="text-xs text-[var(--gowl-text-subtle)]">{item.description}</div>
                      )}
                    </div>
                    <ConfirmButton
                      variant="destructive"
                      size="sm"
                      title={COPY.deleteReferenceTitle}
                      onConfirm={() => onChange(removeReferenceDatum(model, item.id))}
                    >
                      {COPY.delete}
                    </ConfirmButton>
                  </li>
                ))}
              </ul>
            </div>
          </TabsContent>

          <TabsContent value="sources">
            <div className="flex flex-col gap-3">
              <p className="text-xs text-[var(--gowl-text-subtle)]">{COPY.sourcesHelp}</p>
              <div className="flex flex-wrap gap-2 rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] p-2">
                {model.sources.map((source) => (
                  <span
                    key={source.id}
                    className="inline-flex items-center gap-1 rounded-full bg-[var(--gowl-fill)] px-2.5 py-1 text-xs text-[var(--gowl-text)]"
                  >
                    {source.displayName}
                    <button
                      type="button"
                      aria-label={COPY.removeSource(source.displayName)}
                      onClick={() => removeSource(source.id)}
                      className="text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]"
                    >
                      {COPY.removeGlyph}
                    </button>
                  </span>
                ))}
                <input
                  value={sourceInput}
                  onChange={(e) => setSourceInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addSource(sourceInput);
                    }
                  }}
                  placeholder={COPY.selectOrTypeSource}
                  className="min-w-[140px] flex-1 border-none bg-transparent text-sm text-[var(--gowl-text)] outline-none placeholder:text-[var(--gowl-text-muted)]"
                  list="ontology-builder-source-packs"
                />
                <datalist id="ontology-builder-source-packs">
                  {packs.map((pack) => (
                    <option key={pack.id} value={pack.id}>
                      {pack.name}
                    </option>
                  ))}
                </datalist>
              </div>
            </div>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
}
