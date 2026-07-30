/** Composing a policy and reading its dry-run — Epic 41 Slice F.
 *
 *  `41-ui-workbench-governance.md` names the reason this screen exists: "a policy
 *  saved without preview is a production access change made blind, and this is the
 *  one screen where a mistake is a security incident."
 *
 *  So the decisions here are about **what the preview means**, not about drawing
 *  it: a policy that admits everything and a correct one look identical in a
 *  count, and a policy that denies everything looks like a working filter until
 *  somebody cannot do their job. */

export type Effect = "allow" | "deny";

export type Operation =
  | "viewBasic"
  | "viewDetails"
  | "viewSensitive"
  | "create"
  | "editDescription"
  | "editTags"
  | "editOwners"
  | "delete"
  | "restore";

/** What a rule applies to. Mirrors `graph_owl_authz::ResourceMatcher`, which is
 *  deliberately coarse: "a matcher that can express anything is one nobody can
 *  reason about, and 'what can Asha see' has to be answerable by reading the
 *  policies." */
export type Matcher =
  | { readonly type: "all" }
  | { readonly type: "fqnPrefix"; readonly value: string }
  | { readonly type: "tagged"; readonly value: string };

export interface Rule {
  readonly name: string;
  readonly effect: Effect;
  readonly operations: readonly Operation[];
  readonly resources: Matcher;
}

export interface Policy {
  readonly name: string;
  readonly rules: readonly Rule[];
}

/** What the form holds while somebody is composing. */
export interface Draft {
  readonly name: string;
  readonly ruleName: string;
  readonly effect: Effect;
  readonly operations: readonly Operation[];
  readonly matcherType: Matcher["type"];
  readonly matcherValue: string;
}

/** What a dry-run returned. */
export interface DryRun {
  readonly admitted: number;
  readonly denied: number;
  readonly total: number;
  readonly examples: readonly string[];
  readonly admitsEverything: boolean;
}

/** How the preview should be read, and whether it is safe to act on. */
export interface Verdict {
  /** Worth saying out loud above the numbers. */
  readonly warnings: readonly string[];
  /** Nothing in the estate is visible under this policy. */
  readonly deniesEverything: boolean;
}

export function toPolicy(draft: Draft): Policy {
  const value = draft.matcherValue.trim();
  // `all` carries no value. Sending one anyway would be a field the server does
  // not read, which reads as configuration that quietly does nothing.
  const resources: Matcher =
    draft.matcherType === "all" ? { type: "all" } : { type: draft.matcherType, value };
  return {
    name: draft.name.trim(),
    rules: [
      {
        name: draft.ruleName.trim(),
        effect: draft.effect,
        operations: draft.operations,
        resources,
      },
    ],
  };
}

/** What is still missing, in the words the form labels use.
 *
 *  Returned as a list rather than a boolean so the screen can say *which* field,
 *  and so a disabled Save button has a reason attached to it. */
export function incomplete(draft: Draft): string[] {
  const missing: string[] = [];
  if (draft.name.trim() === "") missing.push("name");
  if (draft.ruleName.trim() === "") missing.push("rule name");
  // **The dangerous empty.** A rule with no operations applies to nothing, so a
  // dry-run reports it as harmless — and it would be saved as a rule that looks
  // like protection and is not.
  if (draft.operations.length === 0) missing.push("operations");
  if (draft.matcherType !== "all" && draft.matcherValue.trim() === "") {
    missing.push(draft.matcherType === "tagged" ? "tag" : "resource prefix");
  }
  return missing;
}

export function verdict(run: DryRun): Verdict {
  const warnings: string[] = [];

  // An empty estate cannot tell you anything about a policy. Checked first,
  // because "admits nothing" is technically true here and would blame the policy
  // for an empty catalog.
  if (run.total === 0) {
    warnings.push(
      "There are no assets to preview against, so this says nothing about what the policy would do.",
    );
  }

  if (run.admitsEverything) {
    warnings.push(
      "This policy admits everything. That is almost always a mistake, and it looks identical to a correct policy in the counts alone.",
    );
  }

  const deniesEverything = run.total > 0 && run.admitted === 0;
  if (deniesEverything) {
    warnings.push(
      "This policy admits nothing. It will look like a working filter until somebody cannot do their job.",
    );
  }

  return { warnings, deniesEverything };
}
