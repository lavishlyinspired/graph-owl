/** The ontology diagram canvas.
 *
 *  Renders entity types as coloured nodes and relationships as directed
 *  edges. Supports radial, tree, and force layouts, plus polyline and
 *  orthogonal edge styles. Selecting a node or edge reports it upstream. */

import { useEffect, useRef } from "react";
import cytoscape from "cytoscape";
import { palette } from "../../theme";
import type { OntologyElement } from "./cytoscapeModel";

export type LayoutName = "radial" | "tree" | "force";
export type EdgeStyle = "polyline" | "orthogonal";

type Colors = (typeof palette)["light"];

interface OntologyCanvasProps {
  readonly elements: readonly OntologyElement[];
  readonly colors: Colors;
  readonly layout: LayoutName;
  readonly edgeStyle: EdgeStyle;
  readonly selectedId: string | null;
  readonly onSelectNode: (id: string) => void;
  readonly onSelectEdge: (id: string) => void;
  readonly onClearSelection: () => void;
}

function layoutOptions(name: LayoutName): cytoscape.LayoutOptions {
  switch (name) {
    case "radial":
      return { name: "concentric", minNodeSpacing: 60, animate: false } as cytoscape.LayoutOptions;
    case "tree":
      return {
        name: "breadthfirst",
        directed: true,
        animate: false,
        spacingFactor: 1.2,
        padding: 24,
      } as cytoscape.LayoutOptions;
    case "force":
      return {
        name: "cose",
        animate: false,
        padding: 24,
        componentSpacing: 80,
        nodeRepulsion: 8000,
      } as cytoscape.LayoutOptions;
    default:
      return { name: "breadthfirst", directed: true, animate: false } as cytoscape.LayoutOptions;
  }
}

export function OntologyCanvas({
  elements,
  colors,
  layout,
  edgeStyle,
  selectedId,
  onSelectNode,
  onSelectEdge,
  onClearSelection,
}: OntologyCanvasProps) {
  const host = useRef<HTMLDivElement | null>(null);
  const cy = useRef<cytoscape.Core | null>(null);

  useEffect(() => {
    if (!host.current) return undefined;
    const instance = cytoscape({
      container: host.current,
      elements: elements as cytoscape.ElementDefinition[],
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "font-size": 12,
            color: colors.text,
            "text-valign": "bottom",
            "text-margin-y": 6,
            "background-color": "data(color)",
            width: 32,
            height: 32,
            "border-width": 2,
            "border-color": colors.raised,
          },
        },
        {
          selector: "node:selected",
          style: {
            "border-color": colors.primary,
            "border-width": "4",
          },
        },
        {
          selector: "edge",
          style: {
            width: 1.5,
            "line-color": colors.textSubtle,
            "target-arrow-color": colors.textSubtle,
            "target-arrow-shape": "triangle",
            "curve-style": edgeStyle === "orthogonal" ? "segments" : "straight",
            label: "data(label)",
            "font-size": 10,
            color: colors.textMuted,
            "text-background-color": colors.raised,
            "text-background-opacity": 1,
            "text-background-padding": "2",
            "text-background-shape": "roundrectangle",
          },
        },
        {
          selector: "edge.self-loop",
          style: {
            "curve-style": "bezier",
          },
        },
      ],
      layout: layoutOptions(layout),
      autoungrabify: true,
      selectionType: "single",
    });

    cy.current = instance;

    instance.on("tap", "node", (event) => {
      onSelectNode(event.target.id() as string);
    });
    instance.on("tap", "edge", (event) => {
      onSelectEdge(event.target.id() as string);
    });
    instance.on("tap", (event) => {
      if (event.target === instance) onClearSelection();
    });

    return () => {
      instance.destroy();
      cy.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    instance.elements().remove();
    instance.add(elements as cytoscape.ElementDefinition[]);
    instance.layout(layoutOptions(layout)).run();
    if (selectedId) {
      const selected = instance.getElementById(selectedId);
      if (selected) selected.select();
    }
  }, [elements, layout, selectedId]);

  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    instance.edges().style({
      "curve-style": edgeStyle === "orthogonal" ? "segments" : "straight",
    });
  }, [edgeStyle]);

  return (
    <div
      ref={host}
      role="img"
      aria-label="Ontology diagram showing entity types and relationships"
      style={{
        width: "100%",
        height: "100%",
        background: colors.surface,
        borderRadius: 16,
      }}
    />
  );
}
