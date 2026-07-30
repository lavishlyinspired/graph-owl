import { describe, expect, it } from "vitest";
import { type Draft, type DryRun, incomplete, toPolicy, verdict } from "./policy";

function draft(overrides: Partial<Draft> = {}): Draft {
  return {
    name: "analyst-read",
    ruleName: "read the warehouse",
    effect: "allow",
    operations: ["viewBasic"],
    matcherType: "fqnPrefix",
    matcherValue: "warehouse.",
    ...overrides,
  };
}

describe("composing a policy from the form", () => {
  it("builds a single-rule policy in the shape the API expects", () => {
    const policy = toPolicy(draft());

    expect(policy).toEqual({
      name: "analyst-read",
      rules: [
        {
          name: "read the warehouse",
          effect: "allow",
          operations: ["viewBasic"],
          resources: { type: "fqnPrefix", value: "warehouse." },
        },
      ],
    });
  });

  // `all` carries no value, and sending one would be a field the server does not
  // read — which reads as configuration that does nothing.
  it("omits the value for an all-resources matcher", () => {
    const policy = toPolicy(draft({ matcherType: "all", matcherValue: "ignored" }));

    expect(policy.rules[0]!.resources).toEqual({ type: "all" });
  });

  // Every trimmed field, not two of three. Mutation found the rule name
  // untrimmed-and-untested: a name saved as "  read  " is a name nobody can match
  // when they come looking for it.
  it("trims whitespace rather than saving something nobody typed", () => {
    const policy = toPolicy(
      draft({ matcherValue: "  warehouse.  ", name: " p ", ruleName: "  read  " }),
    );

    expect(policy.name).toBe("p");
    expect(policy.rules[0]!.name).toBe("read");
    expect(policy.rules[0]!.resources).toEqual({ type: "fqnPrefix", value: "warehouse." });
  });

  // The `tagged` matcher was an entire untested branch — Epic 25's classification
  // is what it exists for, and it carries a value like `fqnPrefix` does.
  it("builds a tagged matcher", () => {
    const policy = toPolicy(draft({ matcherType: "tagged", matcherValue: " pii " }));

    expect(policy.rules[0]!.resources).toEqual({ type: "tagged", value: "pii" });
  });
});

describe("what the form still needs", () => {
  it("is satisfied by a complete draft", () => {
    expect(incomplete(draft())).toEqual([]);
  });

  it("needs a policy name", () => {
    expect(incomplete(draft({ name: "  " }))).toContain("name");
  });

  it("needs a rule name", () => {
    expect(incomplete(draft({ ruleName: "" }))).toContain("rule name");
  });

  // Whitespace is not a name. Asserted separately because an emptiness check that
  // forgets to trim passes the `""` case and lets "   " through.
  it("does not accept whitespace as a rule name", () => {
    expect(incomplete(draft({ ruleName: "   " }))).toContain("rule name");
  });

  // **An operationless rule is the dangerous empty.** It applies to nothing, so a
  // dry-run reports it as harmless — and it would be saved as a rule that looks
  // like protection and is not.
  it("needs at least one operation", () => {
    expect(incomplete(draft({ operations: [] }))).toContain("operations");
  });

  it("needs a value for a prefix matcher", () => {
    expect(incomplete(draft({ matcherType: "fqnPrefix", matcherValue: " " }))).toContain(
      "resource prefix",
    );
  });

  // The tagged branch names the *tag*, not a prefix — telling somebody to supply a
  // "resource prefix" when the field is a tag sends them to the wrong input.
  it("names the tag when a tagged matcher has no value", () => {
    const missing = incomplete(draft({ matcherType: "tagged", matcherValue: "" }));

    expect(missing).toContain("tag");
    expect(missing).not.toContain("resource prefix");
  });

  // And the negative: `all` needs no value, so requiring one would make the
  // simplest correct policy unsubmittable.
  it("needs no value for an all matcher", () => {
    expect(incomplete(draft({ matcherType: "all", matcherValue: "" }))).toEqual([]);
  });
});

describe("reading the dry-run", () => {
  const run = (over: Partial<DryRun> = {}): DryRun => ({
    admitted: 40,
    denied: 84,
    total: 124,
    examples: ["warehouse.retail.public.orders"],
    admitsEverything: false,
    ...over,
  });

  it("says nothing about a policy that admits some of the estate", () => {
    expect(verdict(run()).warnings).toEqual([]);
    expect(verdict(run()).deniesEverything).toBe(false);
  });

  // "A policy that admits everything is almost always a mistake, and it looks
  // identical to a correct one in a count alone."
  it("warns when the policy admits everything", () => {
    const { warnings } = verdict(run({ admitted: 124, denied: 0, admitsEverything: true }));

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("everything");
  });

  // The opposite mistake, and the one that looks like a working filter until
  // somebody cannot do their job.
  it("warns when the policy admits nothing", () => {
    const { warnings, deniesEverything } = verdict(run({ admitted: 0, denied: 124 }));

    expect(deniesEverything).toBe(true);
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("nothing");
  });

  // An empty estate cannot tell you anything about a policy, and reporting
  // "admits nothing" would blame the policy for an empty catalog.
  it("says the preview is uninformative rather than alarming on an empty estate", () => {
    const { warnings, deniesEverything } = verdict(run({ admitted: 0, denied: 0, total: 0 }));

    expect(deniesEverything).toBe(false);
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("no assets");
  });

  // Both flags at once is possible — `admitsEverything` is the server's own
  // verdict, not derived from the counts — and each is worth saying.
  it("reports both problems when both apply", () => {
    const { warnings } = verdict(run({ admitted: 0, denied: 0, total: 0, admitsEverything: true }));

    expect(warnings.length).toBeGreaterThanOrEqual(2);
  });
});
