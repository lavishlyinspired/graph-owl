/** Turning a flat team list into a tree — Epic 41 Slice F, principal management.
 *
 *  `00f-ui-architecture.md` puts the part that can be wrong in a pure function.
 *  For a team hierarchy that is entirely the *shape*: a parent that is not in the
 *  list, and a cycle in the data. Both render as something plausible if handled
 *  carelessly — an orphan silently becomes a root, and a cycle hangs the renderer
 *  — so both are decided here where they can be tested without drawing anything.
 *
 *  **The server refuses cycles (Epic 11 Slice B) and this still guards against
 *  them.** A contract is a claim about one server, not a guarantee about whichever
 *  one is answering: the same assumption crashed the asset page when an older
 *  build omitted `owners`. A hang is worse than a crash, because there is nothing
 *  in the console to report. */

import type { Team } from "../api";

export interface TeamNode {
  readonly team: Team;
  readonly children: readonly TeamNode[];
  /** Nesting depth, 0 for a root. Carried so a renderer can indent without
   *  recomputing it, and so a test can assert the shape without walking. */
  readonly depth: number;
}

export interface Hierarchy {
  readonly roots: readonly TeamNode[];
  /** Teams whose `parentTeamId` names a team not in the list.
   *
   *  **Surfaced rather than promoted to a root.** "Reports into a team you cannot
   *  see" is a real fact — a permissions filter, or a parent deleted between two
   *  reads — and drawing it at the top level asserts it is top-level, which is a
   *  different and wrong statement. */
  readonly orphans: readonly Team[];
  /** Ids that could not be placed because following their parents led back to
   *  themselves. Reported so the admin screen can say so instead of hanging. */
  readonly cyclic: readonly string[];
}

export function hierarchy(teams: readonly Team[]): Hierarchy {
  const byId = new Map(teams.map((team) => [team.id, team]));

  /** Does following this team's parents terminate at a root, or come back here?
   *
   *  Walked per team rather than by detecting strongly-connected components: the
   *  set is small, and this also catches a team that merely *reports into* a cycle
   *  without being part of one — which would otherwise be placed under a parent
   *  that is itself unplaceable. */
  const classify = (start: Team): "rooted" | "orphan" | "cyclic" => {
    // Starts empty on purpose. Seeding it with `start.id` looks like it is what
    // catches self-parenting, and it is not: the loop records each parent as it
    // walks, so `a → a` is caught on the second pass either way. Mutation found the
    // seed by emptying it and breaking nothing — dead code defended by a plausible
    // explanation, which is the kind this project keeps finding.
    const seen = new Set<string>();
    let current = start;
    for (;;) {
      const parentId = current.parentTeamId;
      if (parentId === null) return "rooted";
      // The bound is the walk's own history, not a hop count: a limit would need a
      // number, and any number is either wrong for a deep org or a magic constant.
      if (seen.has(parentId)) return "cyclic";
      const parent = byId.get(parentId);
      if (parent === undefined) return "orphan";
      seen.add(parentId);
      current = parent;
    }
  };

  const rooted: Team[] = [];
  const orphans: Team[] = [];
  const cyclic: string[] = [];
  for (const team of teams) {
    const verdict = classify(team);
    if (verdict === "rooted") rooted.push(team);
    else if (verdict === "orphan") orphans.push(team);
    else cyclic.push(team.id);
  }

  // Sorted once, so every level comes out in display-name order without each
  // recursion re-sorting the whole set. `localeCompare` rather than `<` because
  // team names are human text and byte order puts "Zebra" before "apple".
  const ordered = [...rooted].sort((a, b) => a.displayName.localeCompare(b.displayName));

  // One recursion, one node per team. The first version of this built each subtree
  // twice — once to find the children and again to set their depth — which is
  // exponential in depth and produced the right answer, so no test would have
  // caught it.
  const build = (team: Team, depth: number): TeamNode => ({
    team,
    depth,
    children: ordered
      .filter((candidate) => candidate.parentTeamId === team.id)
      .map((child) => build(child, depth + 1)),
  });

  return {
    roots: ordered.filter((team) => team.parentTeamId === null).map((root) => build(root, 0)),
    orphans,
    cyclic,
  };
}
