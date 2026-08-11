/** The findings queue's own decisions — Epic 105 P5.
 *
 *  `ReviewQueue.tsx` is generic and already tested; what is worth pinning here
 *  is the part this config owns: how a finding becomes a list row, and that
 *  the row stays readable for the shapes a real pack produces. */

import { describe, expect, it } from "vitest";
import type { EvidenceGraph, EvidenceGraphEdge, PackFinding } from "../../api";
import {
  describeEvidenceEdge,
  displayTerm,
  evidenceGraphIsJustTheSeed,
  evidencePicture,
  evidenceTriples,
  toQueueEntry,
} from "./findingsQueue";

function getFinding(overrides: Partial<PackFinding> = {}): PackFinding {
  return {
    id: "b0a1c2d3-0000-4000-8000-000000000001",
    pack: "gst",
    label: "https://graph-owl.dev/packs/gst#MissingInGstr2b",
    subject: "https://graph-owl.dev/packs/gst#pr-INV-1003",
    summary: "An invoice claimed in the purchase register that the supplier never filed",
    governedBy: "gst:Section16",
    evidence: [
      { subject: "s", predicate: "https://graph-owl.dev/packs/gst#taxAmount", value: "45000.00" },
    ],
    status: "pending",
    detectedAt: "2026-08-10T09:00:00Z",
    ...overrides,
  };
}

describe("displayTerm", () => {
  it("shows the local name so a reviewer reads a term rather than an IRI", () => {
    expect(displayTerm("https://graph-owl.dev/packs/gst#MissingInGstr2b")).toBe(
      "MissingInGstr2b",
    );
  });

  it("takes the last separator, so a fragment wins over the path before it", () => {
    expect(displayTerm("https://example.org/a/b#c")).toBe("c");
  });

  it("handles a slash-terminated vocabulary, which is as common as a hash one", () => {
    expect(displayTerm("https://example.org/ns/InspectionOverdue")).toBe(
      "InspectionOverdue",
    );
  });

  it("falls back to the whole term rather than rendering a blank row", () => {
    // A subject with no separator is unusual; a blank row in a review queue
    // is unusable, and would look like a bug in the queue rather than in the
    // data.
    expect(displayTerm("bare")).toBe("bare");
    expect(displayTerm("https://example.org/ns#")).toBe("https://example.org/ns#");
  });
});

describe("toQueueEntry", () => {
  it("leads with what kind of finding it is", () => {
    expect(toQueueEntry(getFinding()).summary).toBe("MissingInGstr2b");
  });

  it("names the pack in the detail line, because one queue serves every pack", () => {
    const entry = toQueueEntry(getFinding());

    expect(entry.detail).toContain("gst");
    expect(entry.detail).toContain("pr-INV-1003");
    expect(entry.detail).toContain("never filed");
  });

  it("shows no decision summary while a finding is still pending", () => {
    expect(toQueueEntry(getFinding()).decidedSummary).toBeUndefined();
  });

  it("names who decided once somebody has", () => {
    const entry = toQueueEntry(
      getFinding({ status: "rejected", decidedBy: "asha", reason: "filed late" }),
    );

    expect(entry.decidedSummary).toBe("rejected by asha");
    expect(entry.reason).toBe("filed late");
  });

  it("survives a decided finding whose decider is absent", () => {
    // Nullable on the wire, and a queue that rendered "rejected by null" would
    // look like a data-integrity problem to the person reading it.
    const entry = toQueueEntry(getFinding({ status: "accepted", decidedBy: null }));

    expect(entry.decidedSummary).toBe("accepted");
    expect(entry.reason).toBeUndefined();
  });

  it("renders a hospitality finding identically — the neutrality claim", () => {
    const entry = toQueueEntry(
      getFinding({
        pack: "hospitality",
        label: "https://example.org/hospitality#DuplicateGuest",
        subject: "https://example.org/hospitality#guest-1",
        summary: "Two records for one person",
      }),
    );

    expect(entry.summary).toBe("DuplicateGuest");
    expect(entry.detail).toContain("hospitality");
  });
});

function getEdge(overrides: Partial<EvidenceGraphEdge> = {}): EvidenceGraphEdge {
  return {
    from: "https://graph-owl.dev/packs/gst#pr-INV-1003",
    to: "https://graph-owl.dev/packs/gst#supplier-29AACCG0527D1Z8",
    relationship: "issuedBy",
    ...overrides,
  };
}

describe("describeEvidenceEdge", () => {
  it("reads as a sentence a reviewer can follow, local names not IRIs", () => {
    expect(describeEvidenceEdge(getEdge())).toBe(
      "pr-INV-1003 —issuedBy→ supplier-29AACCG0527D1Z8",
    );
  });

  it("names the relationship by its local name even when it is a full IRI", () => {
    // A derived edge's relationship can arrive as a full predicate IRI; a
    // reviewer reads the local name the same way they read a node's.
    expect(
      describeEvidenceEdge(
        getEdge({ relationship: "https://graph-owl.dev/packs/gst#issuedBy" }),
      ),
    ).toBe("pr-INV-1003 —issuedBy→ supplier-29AACCG0527D1Z8");
  });
});

describe("evidenceGraphIsJustTheSeed", () => {
  it("is true when the walk found nothing beyond the finding's own subject", () => {
    const graph: EvidenceGraph = {
      nodes: [{ id: "pr-INV-1003", iri: null }],
      edges: [],
      truncated: false,
    };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(true);
  });

  it("is false once the walk reaches a second node", () => {
    const graph: EvidenceGraph = {
      nodes: [
        { id: "pr-INV-1003", iri: null },
        { id: "supplier-29AACCG0527D1Z8", iri: null },
      ],
      edges: [getEdge()],
      truncated: false,
    };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(false);
  });

  it("is true for an empty graph too — a finding whose subject failed to resolve", () => {
    const graph: EvidenceGraph = { nodes: [], edges: [], truncated: false };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(true);
  });
});

describe("evidencePicture", () => {
  const graph: EvidenceGraph = {
    nodes: [
      { id: "pr-INV-1003", iri: "https://graph-owl.dev/packs/gst#pr-INV-1003" },
      {
        id: "supplier-29AACCG0527D1Z8",
        iri: "https://graph-owl.dev/packs/gst#supplier-29AACCG0527D1Z8",
      },
    ],
    edges: [getEdge()],
    truncated: false,
  };

  it("seeds the picture at the finding's own subject, by local name", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.seedId).toBe("pr-INV-1003");
  });

  it("carries every node through, named by its resolved IRI's local part", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.nodes).toEqual([
      { id: "pr-INV-1003", name: "pr-INV-1003", kind: null },
      { id: "supplier-29AACCG0527D1Z8", name: "supplier-29AACCG0527D1Z8", kind: null },
    ]);
  });

  it("falls back to the bare id when a node's namespace never resolved to an IRI", () => {
    const unresolved: EvidenceGraph = {
      nodes: [{ id: "pr-INV-1003", iri: null }],
      edges: [],
      truncated: false,
    };
    const picture = evidencePicture(getFinding(), unresolved);
    expect(picture.nodes).toEqual([{ id: "pr-INV-1003", name: "pr-INV-1003", kind: null }]);
  });

  it("carries the edges through unchanged — GraphEdge and EvidenceGraphEdge already share a shape", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.edges).toEqual(graph.edges);
  });

  it("treats every node as already expanded — nothing here is a click-to-reveal", () => {
    // The evidence graph is fetched whole in one call; a `.expandable` ring
    // on any node would promise a click that does nothing.
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.expanded).toEqual(["pr-INV-1003", "supplier-29AACCG0527D1Z8"]);
  });

  it("marks the seed as truncated when the walk hit its budget", () => {
    const picture = evidencePicture(getFinding(), { ...graph, truncated: true });
    expect(picture.truncatedAt).toEqual(["pr-INV-1003"]);
  });

  it("marks nothing as truncated when the walk completed", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.truncatedAt).toEqual([]);
  });
});

describe("evidenceTriples", () => {
  it("reads each edge as a subject/predicate/object row, local names throughout", () => {
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [getEdge({ derived: true })],
      truncated: false,
    };
    expect(evidenceTriples(graph)).toEqual([
      {
        subject: "pr-INV-1003",
        predicate: "issuedBy",
        object: "supplier-29AACCG0527D1Z8",
        derived: true,
      },
    ]);
  });

  it("defaults derived to false when the server did not send it", () => {
    const graph: EvidenceGraph = { nodes: [], edges: [getEdge()], truncated: false };
    const [row] = evidenceTriples(graph);
    expect(row?.derived).toBe(false);
  });

  it("is empty for a graph with no edges", () => {
    expect(evidenceTriples({ nodes: [], edges: [], truncated: false })).toEqual([]);
  });
});
