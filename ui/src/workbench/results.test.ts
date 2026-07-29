import { describe, expect, it } from "vitest";
import {
  type Solution,
  columns,
  display,
  graphShape,
  toGraph,
  verdict,
} from "./results";

const triples: Solution[] = [
  { s: "<https://x/orders>", p: "<https://x/feeds>", o: "<https://x/mart>" },
  { s: "<https://x/mart>", p: "<https://x/feeds>", o: "<https://x/report>" },
];

describe("columns come from the solutions", () => {
  // `SELECT ?name ?owner` order is not in the response, so it is recovered from
  // first appearance. Sorting alphabetically silently rewrites the shape the
  // author wrote.
  it("keeps first-appearance order rather than sorting", () => {
    expect(columns([{ name: "orders", owner: "finance" }])).toEqual(["name", "owner"]);
    expect(columns([{ zebra: "1", apple: "2" }])).toEqual(["zebra", "apple"]);
  });

  // OPTIONAL binds a variable in some solutions and not others. Reading only
  // the first row loses the column entirely, and with it every value in it.
  it("finds a variable that only later solutions bind", () => {
    const rows: Solution[] = [{ name: "orders" }, { name: "mart", owner: "finance" }];

    expect(columns(rows)).toEqual(["name", "owner"]);
  });

  it("does not repeat a variable bound by every solution", () => {
    expect(columns([{ n: "a" }, { n: "b" }, { n: "c" }])).toEqual(["n"]);
  });

  it("has no columns for no solutions", () => {
    expect(columns([])).toEqual([]);
  });
});

describe("the graph view is offered only when it would be honest", () => {
  // The failure mode this guards: two arbitrary columns rendered as nodes and
  // an edge assert a relationship the query never returned, and a reader
  // believes a picture more readily than a table.
  it("refuses results that are not triples", () => {
    expect(graphShape([{ name: "orders", rowCount: "42" }])).toBeNull();
  });

  it("recognises the spellings people actually write", () => {
    expect(graphShape([{ s: "a", p: "b", o: "c" }])).toEqual({
      subject: "s",
      predicate: "p",
      object: "o",
    });
    expect(graphShape([{ subject: "a", predicate: "b", object: "c" }])).toMatchObject({
      subject: "subject",
    });
    expect(graphShape([{ from: "a", relationship: "b", to: "c" }])).toMatchObject({
      subject: "from",
    });
  });

  // One row short of a triple is a row that would vanish from the picture, and
  // a graph missing rows the table shows is worse than no graph.
  //
  // Each position independently: a check that only looked at two of the three
  // still refuses the row missing the third, and looks correct doing it.
  it.each(["s", "p", "o"])("refuses when a solution is missing %s", (missing) => {
    const complete: Solution = { s: "a", p: "b", o: "c" };
    const partial: Solution = Object.fromEntries(
      Object.entries(complete).filter(([k]) => k !== missing),
    );

    expect(graphShape([complete, partial])).toBeNull();
  });

  // And the same for a lone solution, so the refusal is about the missing
  // position rather than about disagreement between rows.
  it.each(["s", "p", "o"])("refuses a single solution missing %s", (missing) => {
    const partial: Solution = Object.fromEntries(
      Object.entries({ s: "a", p: "b", o: "c" }).filter(([k]) => k !== missing),
    );

    expect(graphShape([partial])).toBeNull();
  });

  it("refuses an empty result set", () => {
    expect(graphShape([])).toBeNull();
  });

  // Extra columns alongside a triple are fine — the triple is still there.
  it("accepts a triple carrying extra variables", () => {
    expect(graphShape([{ s: "a", p: "b", o: "c", confidence: "0.9" }])).not.toBeNull();
  });
});

describe("solutions as a graph", () => {
  const shape = { subject: "s", predicate: "p", object: "o" };

  // A subject in ten solutions is one node with ten edges, not ten overlapping
  // nodes — which is what an un-deduplicated render looks like.
  it("draws a node once however often it appears", () => {
    const graph = toGraph(triples, shape);

    expect(graph.nodes.map((n) => n.id)).toEqual([
      "<https://x/orders>",
      "<https://x/mart>",
      "<https://x/report>",
    ]);
  });

  it("keeps one edge per solution", () => {
    expect(toGraph(triples, shape).edges).toHaveLength(2);
  });

  // Two identical triples in a result set is something the reader should see,
  // not something this quietly hides.
  it("does not collapse duplicate edges", () => {
    const twice = [triples[0]!, triples[0]!];

    expect(toGraph(twice, shape).edges).toHaveLength(2);
  });

  it("labels edges with the predicate", () => {
    expect(toGraph(triples, shape).edges[0]!.label).toBe("feeds");
  });

  it("draws nothing from no solutions", () => {
    expect(toGraph([], shape)).toEqual({ nodes: [], edges: [] });
  });

  // `toGraph` is public and a caller can hand it a shape that does not fit —
  // and each position matters separately. A row missing any one of the three
  // is skipped rather than drawn with an `undefined` endpoint, which renders
  // as a node named "undefined" wired to everything else that lost the same
  // position.
  it.each(["s", "p", "o"])("skips a row that does not bind %s", (missing) => {
    const partial: Solution = Object.fromEntries(
      Object.entries({ s: "a", p: "b", o: "c" }).filter(([k]) => k !== missing),
    );

    const graph = toGraph([partial], shape);

    expect(graph.edges).toEqual([]);
    expect(graph.nodes).toEqual([]);
  });

  // And the negative, so the three above are about the missing position rather
  // than about a function that draws nothing.
  it("still draws a row that binds all three", () => {
    const graph = toGraph([{ s: "a", p: "b", o: "c" }], shape);

    expect(graph.edges).toHaveLength(1);
    expect(graph.nodes).toHaveLength(2);
  });
});

describe("a term as a reader reads it", () => {
  // A results graph of full IRIs is one where every label is the same eighty
  // characters and the last six carry the meaning.
  it("shows the local part of an IRI", () => {
    expect(display("<https://graph-owl.dev/ns/catalog#name>")).toBe("name");
    expect(display("https://x/orders")).toBe("orders");
  });

  it("leaves a plain literal alone", () => {
    expect(display("orders")).toBe("orders");
  });

  // The angle brackets are stripped only where SPARQL puts them: at the ends.
  // An unanchored strip would eat a `<` inside a literal, silently rewriting
  // the value a reader is looking at.
  it("strips angle brackets only at the ends", () => {
    expect(display("a<b")).toBe("a<b");
    expect(display("a>b")).toBe("a>b");
    expect(display(">leading")).toBe(">leading");
    expect(display("trailing<")).toBe("trailing<");
  });

  // A term ending in its separator would otherwise render as an empty label —
  // a node with no name at all.
  it("does not shorten a term into nothing", () => {
    expect(display("https://x/")).toBe("https://x/");
    expect(display("<https://x#>")).toBe("https://x#");
  });
});

describe("what a reader is told before trusting the answer", () => {
  const clean = { truncated: false, factsScanned: 12, plan: ["? 1:name ?"] };

  // Truncation first, because it is the one that makes a wrong answer look
  // like a complete one.
  it("says so when the budget cut the answer short", () => {
    const { warnings } = verdict(triples, { ...clean, truncated: true });

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("cut this answer short");
  });

  // A full scan is a cost, not an error. Saying nothing lets the most
  // expensive query in the system look identical to the cheapest.
  it("says when the whole graph had to be read", () => {
    const { warnings } = verdict(triples, { ...clean, plan: ["? ? ?"] });

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("whole graph");
    expect(warnings[0]).toContain("12");
  });

  it("says nothing about a bounded, complete answer", () => {
    expect(verdict(triples, clean).warnings).toEqual([]);
  });

  // **One** unbounded scan among several is still a full read. A check that
  // required *every* scan to be unbounded would stay silent on the query that
  // reads the estate once and narrows twice.
  it("warns when any one scan is unbounded, not only when all are", () => {
    const mixed = { ...clean, plan: ["? 1:name ?", "? ? ?", "1:x ? ?"] };

    expect(verdict(triples, mixed).warnings).toHaveLength(1);
  });

  // The plan is rendered for people and may carry padding. Comparing against
  // the raw string would miss the full scan it is meant to catch.
  it("recognises an unbounded scan despite surrounding whitespace", () => {
    expect(verdict(triples, { ...clean, plan: ["  ? ? ?  "] }).warnings).toHaveLength(1);
  });

  it("reports both problems when both apply", () => {
    const { warnings } = verdict(triples, { truncated: true, factsScanned: 9, plan: ["? ? ?"] });

    expect(warnings).toHaveLength(2);
  });

  it("reports whether the results can be drawn", () => {
    expect(verdict(triples, clean).canDraw).toBe(true);
    expect(verdict([{ name: "orders" }], clean).canDraw).toBe(false);
  });
});
