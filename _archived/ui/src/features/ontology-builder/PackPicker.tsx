/** A visual picker for installed packs, replacing a plain id-keyed dropdown.
 *
 *  Two things a bare `Select` could not show: what a pack actually IS (its
 *  own installed ontology, not an opaque id) and whether picking it does
 *  anything — a pack with no ontology imported yet is shown, not hidden,
 *  but cannot be loaded. Picking a pack that has one replaces the canvas
 *  with that pack's real ontology, so "what does the GST pack look like"
 *  is a click, not a separate screen. */

import { useState } from "react";
import { Button } from "../../components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import { packCards } from "./packCards";
import type { InstalledPack } from "../packs/packSurfaces";
import type { LoadedSource } from "../packs/packData";

const COPY = {
  placeholder: "Select a pack",
  title: "Choose a pack",
  subtitle: "Loads that pack's own installed ontology as a starting diagram you can extend.",
  noOntology: "No ontology installed yet",
  hasOntology: "Ontology available",
  empty: "No packs installed yet.",
  selectedGlyph: "✓",
};

interface PackPickerProps {
  readonly packs: readonly InstalledPack[];
  readonly sources: readonly LoadedSource[];
  readonly selectedPackId: string | null;
  readonly loading: boolean;
  readonly onSelect: (packId: string) => void;
}

function DatabaseIcon() {
  return (
    <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8">
      <ellipse cx="12" cy="5" rx="8" ry="3" />
      <path d="M4 5v14c0 1.66 3.58 3 8 3s8-1.34 8-3V5" />
      <path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" />
    </svg>
  );
}

export function PackPicker({ packs, sources, selectedPackId, loading, onSelect }: PackPickerProps) {
  const [open, setOpen] = useState(false);
  const cards = packCards(packs, sources);
  const selected = cards.find((card) => card.packId === selectedPackId) ?? null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" disabled={loading} className="gap-2">
          <DatabaseIcon />
          {selected ? selected.label : COPY.placeholder}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80">
        <h5 className="m-0 text-sm font-semibold text-[var(--gowl-text)]">{COPY.title}</h5>
        <p className="mt-1 text-xs text-[var(--gowl-text-subtle)]">{COPY.subtitle}</p>
        <div className="mt-3 flex flex-col gap-2">
          {cards.length === 0 && <p className="text-sm text-[var(--gowl-text-subtle)]">{COPY.empty}</p>}
          {cards.map((card) => {
            const isSelected = card.packId === selectedPackId;
            return (
              <button
                key={card.packId}
                type="button"
                aria-label={card.label}
                disabled={!card.hasOntology}
                onClick={() => {
                  onSelect(card.packId);
                  setOpen(false);
                }}
                className="flex items-center justify-between gap-3 rounded-[var(--gowl-radius-control)] border px-3 py-2 text-left disabled:cursor-not-allowed disabled:opacity-55"
                style={{
                  borderColor: isSelected ? "var(--gowl-primary)" : "var(--gowl-border)",
                  background: isSelected ? "color-mix(in srgb, var(--gowl-primary) 8%, transparent)" : "var(--gowl-raised)",
                  cursor: card.hasOntology ? "pointer" : "not-allowed",
                }}
              >
                <span className="flex items-center gap-2">
                  <span className="text-[var(--gowl-primary)]">
                    <DatabaseIcon />
                  </span>
                  <span>
                    <span className="block text-sm font-semibold text-[var(--gowl-text)]">{card.label}</span>
                    <span className="block text-xs text-[var(--gowl-text-subtle)]">
                      {card.hasOntology ? COPY.hasOntology : COPY.noOntology}
                    </span>
                  </span>
                </span>
                {isSelected && (
                  <span aria-hidden className="text-[var(--gowl-primary)]">
                    {COPY.selectedGlyph}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </PopoverContent>
    </Popover>
  );
}
