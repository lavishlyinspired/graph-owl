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
  evidenceCandidates,
  evidenceNearMiss,
  evidenceNodeSources,
  evidencePicture,
  evidenceTriples,
  humanizeTerm,
  titleFor,
  toQueueEntry,
} from "./findingsQueue";

function getFinding(overrides: Partial<PackFinding> = {}): PackFinding {
  return {
    id: "b0a1c2d3-0000-4000-8000-000000000001",
    pack: "gst",
    label: "https://graph-owl.dev/packs/gst#MissingInGstr2b",
    subject: "https://graph-owl.dev/packs/gst#pr-INV-1003",
    summary:
      "An invoice claimed in the purchase register that the supplier never filed",
    governedBy: "gst:Section16",
    evidence: [
      {
        subject: "s",
        predicate: "https://graph-owl.dev/packs/gst#taxAmount",
        value: "45000.00",
      },
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

  it("cuts a curie's prefix the same way it cuts an IRI's path", () => {
    // A rule's `governedBy` arrives as `gst:Section16`, not as a full IRI —
    // the registry emits curies, and a reviewer reads the name, not the
    // pack's own prefix on top of it.
    expect(displayTerm("gst:Section16")).toBe("Section16");
    expect(displayTerm("gst:TaxHeadMismatch")).toBe("TaxHeadMismatch");
  });

  it("falls back to the whole term rather than rendering a blank row", () => {
    // A subject with no separator is unusual; a blank row in a review queue
    // is unusable, and would look like a bug in the queue rather than in the
    // data.
    expect(displayTerm("bare")).toBe("bare");
    expect(displayTerm("https://example.org/ns#")).toBe(
      "https://example.org/ns#",
    );
  });
});

describe("toQueueEntry", () => {
  it("leads with what kind of finding it is, read as a sentence not an identifier", () => {
    // The row's badge is for a reviewer deciding *whether to open this*, so
    // it says what the finding is in words a reviewer can scan, not the
    // registry's local name ("MissingInGstr2b") it arrived as.
    expect(toQueueEntry(getFinding()).summary).toBe("Missing In Gstr2b");
  });

  it("uses the pack's own declared wording when the caller supplies a title", () => {
    // The pack owns what a finding is called (`[findings.guidance]`); its
    // words outrank any generic humanizing the console could invent.
    const entry = toQueueEntry(getFinding(), "Supplier has not filed");
    expect(entry.summary).toBe("Supplier has not filed");
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
      getFinding({
        status: "rejected",
        decidedBy: "asha",
        reason: "filed late",
      }),
    );

    expect(entry.decidedSummary).toBe("rejected by asha");
    expect(entry.reason).toBe("filed late");
  });

  it("survives a decided finding whose decider is absent", () => {
    // Nullable on the wire, and a queue that rendered "rejected by null" would
    // look like a data-integrity problem to the person reading it.
    const entry = toQueueEntry(
      getFinding({ status: "accepted", decidedBy: null }),
    );

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

    expect(entry.summary).toBe("Duplicate Guest");
    expect(entry.detail).toContain("hospitality");
  });
});

describe("humanizeTerm", () => {
  it("turns a camelCase local name into a spaced, human-readable title", () => {
    expect(humanizeTerm("SupplierNotFiled")).toBe("Supplier Not Filed");
    expect(humanizeTerm("TaxHeadMismatch")).toBe("Tax Head Mismatch");
  });

  it("keeps an acronym together instead of splitting it across words", () => {
    expect(humanizeTerm("ITCNotAvailable")).toBe("ITC Not Available");
  });

  it("leaves already-spaced wording alone — a pack title is not a camel identifier", () => {
    // A pack's own guidance title ("Supplier has not filed") reaches a
    // reader as-is; re-capitalising or re-spacing it would corrupt the
    // pack's voice.
    expect(humanizeTerm("Supplier has not filed")).toBe("Supplier has not filed");
  });

  it("capitalises the first letter of every word", () => {
    expect(humanizeTerm("supplierNotFiled")).toBe("Supplier Not Filed");
  });

  it("leaves a single-word name alone", () => {
    expect(humanizeTerm("Reversed")).toBe("Reversed");
  });

  it("leaves a term with no camel transitions alone", () => {
    // A bare subject id is not a finding label and reads fine untouched.
    expect(humanizeTerm("2b-INV-1003")).toBe("2b-INV-1003");
  });

  it("handles an empty term without inventing a row", () => {
    expect(humanizeTerm("")).toBe("");
  });
});

describe("titleFor", () => {
  it("prefers the pack's own declared title over a humanized name", () => {
    const guidance = {
      "gst:SupplierNotFiled": { title: "Supplier has not filed" },
    };
    expect(titleFor("gst:SupplierNotFiled", guidance)).toBe("Supplier has not filed");
  });

  it("humanizes the local name when the pack declares no guidance for it", () => {
    expect(titleFor("gst:SupplierNotFiled", undefined)).toBe("Supplier Not Filed");
    expect(titleFor("gst:SupplierNotFiled", {})).toBe("Supplier Not Filed");
  });

  it("falls back to the humanized local name when a full-IRI label has no guidance entry", () => {
    // A pack that emits full IRIs rather than curies still renders — the
    // console never blanks a row over a registry convention.
    expect(
      titleFor("https://graph-owl.dev/packs/gst#MissingInGstr2b", {}),
    ).toBe("Missing In Gstr2b");
  });
});

function getEdge(
  overrides: Partial<EvidenceGraphEdge> = {},
): EvidenceGraphEdge {
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
      nodes: [{ id: "pr-INV-1003", iri: null, sources: [], semanticType: null }],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(true);
  });

  it("is false once the walk reaches a second node", () => {
    const graph: EvidenceGraph = {
      nodes: [
        {
          id: "pr-INV-1003",
          iri: null,
          sources: ["gst-purchase-register"],
          semanticType: null,
        },
        { id: "supplier-29AACCG0527D1Z8", iri: null, sources: [], semanticType: null },
      ],
      edges: [getEdge()],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(false);
  });

  it("is true for an empty graph too — a finding whose subject failed to resolve", () => {
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceGraphIsJustTheSeed(graph)).toBe(true);
  });
});

describe("evidencePicture", () => {
  const graph: EvidenceGraph = {
    nodes: [
      {
        id: "pr-INV-1003",
        iri: "https://graph-owl.dev/packs/gst#pr-INV-1003",
        sources: ["gst-purchase-register"],
        semanticType: "PurchaseInvoice",
      },
      {
        id: "supplier-29AACCG0527D1Z8",
        iri: "https://graph-owl.dev/packs/gst#supplier-29AACCG0527D1Z8",
        sources: ["gst-purchase-register", "gst-gstr2b"],
        semanticType: "Supplier",
      },
    ],
    edges: [getEdge()],
    truncated: false,
    nearMiss: null,
  };

  it("seeds the picture at the finding's own subject, by local name", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.seedId).toBe("pr-INV-1003");
  });

  it("carries every node through, named by its resolved IRI's local part", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.nodes).toEqual([
      { id: "pr-INV-1003", name: "pr-INV-1003", kind: null, semanticType: "PurchaseInvoice" },
      {
        id: "supplier-29AACCG0527D1Z8",
        name: "supplier-29AACCG0527D1Z8",
        kind: null,
        semanticType: "Supplier",
      },
    ]);
  });

  it("falls back to the bare id when a node's namespace never resolved to an IRI", () => {
    const unresolved: EvidenceGraph = {
      nodes: [{ id: "pr-INV-1003", iri: null, sources: [], semanticType: null }],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    const picture = evidencePicture(getFinding(), unresolved);
    expect(picture.nodes).toEqual([
      { id: "pr-INV-1003", name: "pr-INV-1003", kind: null, semanticType: null },
    ]);
  });

  it("carries the edges through unchanged — GraphEdge and EvidenceGraphEdge already share a shape", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.edges).toEqual(graph.edges);
  });

  it("treats every node as already expanded — nothing here is a click-to-reveal", () => {
    // The evidence graph is fetched whole in one call; a `.expandable` ring
    // on any node would promise a click that does nothing.
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.expanded).toEqual([
      "pr-INV-1003",
      "supplier-29AACCG0527D1Z8",
    ]);
  });

  it("marks the seed as truncated when the walk hit its budget", () => {
    const picture = evidencePicture(getFinding(), {
      ...graph,
      truncated: true,
      nearMiss: null,
    });
    expect(picture.truncatedAt).toEqual(["pr-INV-1003"]);
  });

  it("marks nothing as truncated when the walk completed", () => {
    const picture = evidencePicture(getFinding(), graph);
    expect(picture.truncatedAt).toEqual([]);
  });
});

describe("evidenceNodeSources", () => {
  it("names each node by its resolved local part, alongside its source", () => {
    const graph: EvidenceGraph = {
      nodes: [
        {
          id: "pr-INV-1003",
          iri: "https://graph-owl.dev/packs/gst#pr-INV-1003",
          sources: ["gst-purchase-register"],
          semanticType: null,
        },
      ],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceNodeSources(graph)).toEqual([
      {
        id: "pr-INV-1003",
        name: "pr-INV-1003",
        sources: ["gst-purchase-register"],
      },
    ]);
  });

  it("carries every source through for a node claimed by more than one document", () => {
    // Epic 105 P7's provenance half — this is the case a reviewer actually
    // needs the list for: a Supplier the purchase register and GSTR-2B both
    // assert, not the common single-source case.
    const graph: EvidenceGraph = {
      nodes: [
        {
          id: "supplier-29AACCG0527D1Z8",
          iri: null,
          sources: ["gst-purchase-register", "gst-gstr2b"],
          semanticType: null,
        },
      ],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceNodeSources(graph)[0]?.sources).toEqual([
      "gst-purchase-register",
      "gst-gstr2b",
    ]);
  });

  it("falls back to the bare id when a node's namespace never resolved to an IRI", () => {
    const graph: EvidenceGraph = {
      nodes: [{ id: "pr-INV-1003", iri: null, sources: [], semanticType: null }],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceNodeSources(graph)[0]?.name).toBe("pr-INV-1003");
  });

  it("reports an empty source list as empty, not absent — a real answer, not a missing field", () => {
    const graph: EvidenceGraph = {
      nodes: [{ id: "pr-INV-1003", iri: null, sources: [], semanticType: null }],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceNodeSources(graph)[0]?.sources).toEqual([]);
  });

  it("is empty for a graph with no nodes", () => {
    expect(
      evidenceNodeSources({
        nodes: [],
        edges: [],
        truncated: false,
        nearMiss: null,
      }),
    ).toEqual([]);
  });
});

describe("evidenceNearMiss", () => {
  it("names the candidate by its resolved local part, alongside its source", () => {
    // Epic 105 P7's near-miss half — the second Supplier a rule like
    // GstinTransposition suspects is the same entity, resolved by value
    // rather than reached by traversal.
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [],
      truncated: false,
      nearMiss: {
        id: "supplier-27AABCU9603R1ZM",
        iri: "https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM",
        sources: ["gst-gstr2b"],
        semanticType: null,
      },
    };
    // Plan 113 Slice C: `iri` is what `ClickableSubject` needs to open this
    // subject's neighbourhood — `id` alone is a bare local name and cannot be
    // resolved back into an identifier the server understands.
    expect(evidenceNearMiss(graph)).toEqual({
      id: "supplier-27AABCU9603R1ZM",
      iri: "https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM",
      name: "supplier-27AABCU9603R1ZM",
      sources: ["gst-gstr2b"],
    });
  });

  it("is null when the finding's rule has no near-miss candidate", () => {
    // The common case — most findings have no similarity band at all.
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [],
      truncated: false,
      nearMiss: null,
    };
    expect(evidenceNearMiss(graph)).toBeNull();
  });
});

describe("evidenceTriples", () => {
  it("reads each edge as a subject/predicate/object row, local names throughout", () => {
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [getEdge({ derived: true })],
      truncated: false,
      nearMiss: null,
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
    const graph: EvidenceGraph = {
      nodes: [],
      edges: [getEdge()],
      truncated: false,
      nearMiss: null,
    };
    const [row] = evidenceTriples(graph);
    expect(row?.derived).toBe(false);
  });

  it("is empty for a graph with no edges", () => {
    expect(
      evidenceTriples({
        nodes: [],
        edges: [],
        truncated: false,
        nearMiss: null,
      }),
    ).toEqual([]);
  });
});

/** Plan 111 Slice F — the pack's blocking strategies reach the reviewer.
 *
 *  **A different claim from a near miss, and it must read as one.** A near
 *  miss means the rule declared a similarity band and a value matched
 *  exactly; a candidate means a blocking key collided. The first is close to
 *  an assertion, the second an invitation to look. */
describe("evidenceCandidates", () => {
  const graph = (candidates?: EvidenceGraph["candidates"]): EvidenceGraph => ({
    nodes: [],
    edges: [],
    truncated: false,
    nearMiss: null,
    candidates,
  });

  it("names each candidate and reports which strategies agreed", () => {
    const [found] = evidenceCandidates(
      graph([
        {
          id: "2b-INV-1004",
          iri: "https://graph-owl.dev/packs/gst#2b-INV-1004",
          sources: ["gst-gstr2b"],
          semanticType: null,
          by: ["ngram"],
        },
      ]),
    );
    expect(found).toEqual({
      id: "2b-INV-1004",
      iri: "https://graph-owl.dev/packs/gst#2b-INV-1004",
      name: "2b-INV-1004",
      sources: ["gst-gstr2b"],
      by: ["ngram"],
    });
  });

  /** **A console newer than its server, and one older than its pack, must
   *  both render.** The field is absent from a server that predates this
   *  slice, and empty for a pack that declares no blocking — neither is an
   *  error, and reading `undefined` as a failure would blank a panel over a
   *  section that is additive to it. */
  it("treats an absent field exactly as an empty one", () => {
    expect(evidenceCandidates(graph(undefined))).toEqual([]);
    expect(evidenceCandidates(graph([]))).toEqual([]);
  });

  /** A candidate with no strategy named is dropped rather than shown as an
   *  unexplained "might be the same record". The *reason* is the only thing
   *  that makes the row actionable; without it the row is a bare assertion. */
  it("drops a candidate that cannot say why it matched", () => {
    expect(
      evidenceCandidates(
        graph([{ id: "x", iri: null, sources: [], semanticType: null, by: [] }]),
      ),
    ).toEqual([]);
  });
});
