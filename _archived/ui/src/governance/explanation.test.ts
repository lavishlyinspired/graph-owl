import { describe, expect, it } from "vitest";
import { type Explanation, type Fact, depthOf, flatten, rulesUsed } from "./explanation";

function fact(s: string, o: string): Fact {
  return { s, p: "256:type", o, t: 1 };
}

/** `payments` is a `GovernedTable`, two subsumption steps down. */
const depthThree: Explanation = {
  status: "derived",
  chains: [
    {
      rule: "subClassOf",
      premises: [
        {
          status: "derived",
          chains: [
            {
              rule: "subClassOf",
              premises: [
                { status: "asserted", fact: fact("1:payments", "1:PiiTable") },
                { status: "asserted", fact: fact("1:PiiTable", "1:SensitiveTable") },
              ],
            },
          ],
        },
        { status: "asserted", fact: fact("1:SensitiveTable", "1:GovernedTable") },
      ],
    },
  ],
};

describe("a chain flattens for reading", () => {
  // The whole point. A chain rendered one level deep names the derived premise
  // and stops — and why *that* held is the half a reviewer is checking.
  it("expands a derived premise instead of stopping at it", () => {
    const rows = flatten(depthThree);

    expect(rows.map((r) => r.kind)).toEqual([
      "rule",
      "rule",
      "asserted",
      "asserted",
      "asserted",
    ]);
    expect(rows.map((r) => r.depth)).toEqual([0, 1, 2, 2, 1]);
  });

  it("names the rule that fired at each step", () => {
    expect(flatten(depthThree)[0]!.rule).toBe("subClassOf");
  });

  it("reports how deep the reasoning went", () => {
    expect(depthOf(depthThree)).toBe(2);
  });

  it("reports depth zero for a fact somebody asserted", () => {
    expect(depthOf({ status: "asserted", fact: fact("1:a", "1:B") })).toBe(0);
  });

  it("shows an asserted fact as itself", () => {
    const rows = flatten({ status: "asserted", fact: fact("1:a", "1:B") });

    expect(rows).toEqual([{ depth: 0, kind: "asserted", fact: fact("1:a", "1:B") }]);
  });

  it("shows an unknown fact as unknown rather than as an empty chain", () => {
    expect(flatten({ status: "unknown" })).toEqual([{ depth: 0, kind: "unknown" }]);
  });

  // Only reachable through a cyclic ontology. Truncating there turns a
  // modelling error into a chain that merely looks short.
  it("shows a circular premise rather than dropping it", () => {
    const rows = flatten({
      status: "derived",
      chains: [
        {
          rule: "subClassOf",
          premises: [{ status: "circular", fact: fact("1:a", "1:B") }],
        },
      ],
    });

    expect(rows.map((r) => r.kind)).toEqual(["rule", "circular"]);
  });

  it("reads a status it does not recognise as unknown rather than throwing", () => {
    const rows = flatten({ status: "speculative" } as unknown as Explanation);

    expect(rows).toEqual([{ depth: 0, kind: "unknown" }]);
  });
});

describe("a fact provable more than one way", () => {
  const twoRoutes: Explanation = {
    status: "derived",
    chains: [
      { rule: "symmetric", premises: [{ status: "asserted", fact: fact("1:bob", "1:alice") }] },
      {
        rule: "subPropertyOf",
        premises: [{ status: "asserted", fact: fact("1:alice", "1:bob") }],
      },
    ],
  };

  // Showing only the first route hides the one a reviewer may find more
  // convincing — or the one that reveals the rule they wanted to disable.
  it("expands every route, not just the first", () => {
    const rows = flatten(twoRoutes);

    expect(rows.filter((r) => r.kind === "rule").map((r) => r.rule)).toEqual([
      "symmetric",
      "subPropertyOf",
    ]);
  });

  it("numbers the routes so a reader can tell them apart", () => {
    expect(flatten(twoRoutes).filter((r) => r.kind === "rule").map((r) => r.route)).toEqual([
      1, 2,
    ]);
  });

  // "Route 1 of 1" is noise. A single explanation is just the explanation.
  it("does not number a single route", () => {
    expect(flatten(depthThree)[0]!.route).toBeUndefined();
  });

  it("lists each rule that took part, once", () => {
    expect(rulesUsed(twoRoutes)).toEqual(["symmetric", "subPropertyOf"]);
  });

  it("does not repeat a rule that fired twice", () => {
    expect(rulesUsed(depthThree)).toEqual(["subClassOf"]);
  });

  it("lists no rules for a fact nobody derived", () => {
    expect(rulesUsed({ status: "asserted", fact: fact("1:a", "1:B") })).toEqual([]);
  });
});

// Epic 95 added four rules to the reasoner (propertyChain,
// inverseFunctionalProperty, functionalProperty, hasKey) without adding a
// new `Explanation`/`Chain` shape — `rule` is a plain string this module
// never special-cases. So the four new wire names (RuleName's derived
// `Serialize` with `rename_all = "camelCase"`, not the `rule:`-prefixed
// `as_str()` used elsewhere) need no production change here — this proves
// that rather than assuming it, matching the wire values a real server
// response carries.
describe("the four Epic 95 axioms flow through unchanged", () => {
  const newAxiomChain: Explanation = {
    status: "derived",
    chains: [
      {
        rule: "propertyChain",
        premises: [{ status: "asserted", fact: fact("1:orders", "1:warehouse") }],
      },
      {
        rule: "inverseFunctionalProperty",
        premises: [{ status: "asserted", fact: fact("1:acct-1", "1:acct-2") }],
      },
      {
        rule: "functionalProperty",
        premises: [{ status: "asserted", fact: fact("1:table-a", "1:owner-x") }],
      },
      {
        rule: "hasKey",
        premises: [{ status: "asserted", fact: fact("1:cust-1", "1:cust-2") }],
      },
    ],
  };

  it("names each new rule at the step it fired", () => {
    expect(flatten(newAxiomChain).filter((r) => r.kind === "rule").map((r) => r.rule)).toEqual([
      "propertyChain",
      "inverseFunctionalProperty",
      "functionalProperty",
      "hasKey",
    ]);
  });

  it("lists all four in rulesUsed, none dropped as unrecognised", () => {
    expect(rulesUsed(newAxiomChain)).toEqual([
      "propertyChain",
      "inverseFunctionalProperty",
      "functionalProperty",
      "hasKey",
    ]);
  });
});
