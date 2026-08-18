import { describe, expect, it } from "vitest";
import type { Team } from "../api";
import { hierarchy } from "./hierarchy";

function team(id: string, parentTeamId: string | null = null, displayName = id): Team {
  return { id, displayName, description: null, members: [], parentTeamId };
}

/** Ids at each level, so a test asserts shape without walking the tree by hand. */
function shape(nodes: readonly { team: Team; children: readonly unknown[] }[]): string[] {
  return nodes.map((n) => `${n.team.id}(${n.children.length})`);
}

describe("building the team hierarchy", () => {
  it("has nothing to show for no teams", () => {
    const built = hierarchy([]);

    expect(built.roots).toEqual([]);
    expect(built.orphans).toEqual([]);
    expect(built.cyclic).toEqual([]);
  });

  it("puts a team with no parent at the root", () => {
    const built = hierarchy([team("platform")]);

    expect(shape(built.roots)).toEqual(["platform(0)"]);
    expect(built.roots[0]!.depth).toBe(0);
  });

  it("nests a child under its parent", () => {
    const built = hierarchy([team("platform"), team("data-eng", "platform")]);

    expect(shape(built.roots)).toEqual(["platform(1)"]);
    expect(built.roots[0]!.children[0]!.team.id).toBe("data-eng");
    expect(built.roots[0]!.children[0]!.depth).toBe(1);
  });

  // Three levels, because a two-level build can be produced by a single pass that
  // does not recurse and would then flatten anything deeper.
  it("nests three levels deep and reports the depth of each", () => {
    const built = hierarchy([
      team("exec"),
      team("platform", "exec"),
      team("data-eng", "platform"),
    ]);

    const exec = built.roots[0]!;
    const platform = exec.children[0]!;
    const dataEng = platform.children[0]!;

    expect(exec.depth).toBe(0);
    expect(platform.depth).toBe(1);
    expect(dataEng.depth).toBe(2);
    expect(dataEng.children).toEqual([]);
  });

  // Input order must not decide the tree: a child listed before its parent is the
  // normal case for an id-ordered API response.
  it("nests correctly when a child is listed before its parent", () => {
    const built = hierarchy([team("data-eng", "platform"), team("platform")]);

    expect(shape(built.roots)).toEqual(["platform(1)"]);
  });

  it("keeps siblings in display-name order so the tree is stable between reads", () => {
    const built = hierarchy([
      team("a", null, "Zebra"),
      team("b", null, "Apple"),
      team("c", null, "Mango"),
    ]);

    expect(built.roots.map((r) => r.team.displayName)).toEqual(["Apple", "Mango", "Zebra"]);
  });

  it("orders nested siblings too, not only roots", () => {
    const built = hierarchy([
      team("root"),
      team("x", "root", "Zebra"),
      team("y", "root", "Apple"),
    ]);

    expect(built.roots[0]!.children.map((c) => c.team.displayName)).toEqual(["Apple", "Zebra"]);
  });

  // **An orphan is not a root.** Its parent exists somewhere the reader cannot see
  // — filtered by policy, or deleted between two reads — and drawing it at the top
  // level asserts it is top-level, which is a different and wrong statement.
  it("reports a team whose parent is absent as an orphan rather than a root", () => {
    const built = hierarchy([team("platform"), team("stray", "nowhere")]);

    expect(shape(built.roots)).toEqual(["platform(0)"]);
    expect(built.orphans.map((o) => o.id)).toEqual(["stray"]);
  });

  it("does not lose an orphan's own children", () => {
    const built = hierarchy([team("stray", "nowhere"), team("below", "stray")]);

    // The orphan is reported, and `below` is not silently promoted either — it
    // still reports into something outside the visible tree.
    expect(built.roots).toEqual([]);
    expect(built.orphans.map((o) => o.id).sort()).toEqual(["below", "stray"]);
  });

  // **A cycle must not hang the renderer.** The server refuses cycles, but a
  // contract is a claim about one server rather than a guarantee about whichever
  // is answering — and a hang leaves nothing in the console to report.
  it("reports a two-team cycle instead of looping forever", () => {
    const built = hierarchy([team("a", "b"), team("b", "a")]);

    expect(built.roots).toEqual([]);
    expect([...built.cyclic].sort()).toEqual(["a", "b"]);
  });

  it("reports a three-team cycle", () => {
    const built = hierarchy([team("a", "c"), team("b", "a"), team("c", "b")]);

    expect([...built.cyclic].sort()).toEqual(["a", "b", "c"]);
  });

  it("reports a team that is its own parent", () => {
    const built = hierarchy([team("a", "a")]);

    expect(built.cyclic).toEqual(["a"]);
    expect(built.roots).toEqual([]);
  });

  // A cycle elsewhere must not take the healthy part of the tree with it.
  it("still builds the reachable tree when a separate cycle exists", () => {
    const built = hierarchy([
      team("platform"),
      team("data-eng", "platform"),
      team("a", "b"),
      team("b", "a"),
    ]);

    expect(shape(built.roots)).toEqual(["platform(1)"]);
    expect([...built.cyclic].sort()).toEqual(["a", "b"]);
  });

  // Every team lands in exactly one bucket. Nothing may vanish: a team missing
  // from an admin screen is one nobody can fix.
  it("accounts for every team exactly once", () => {
    const teams = [
      team("platform"),
      team("data-eng", "platform"),
      team("stray", "nowhere"),
      team("a", "b"),
      team("b", "a"),
    ];

    const built = hierarchy(teams);
    const placed = (function count(nodes: readonly { children: readonly unknown[] }[]): number {
      return nodes.reduce(
        (total, n) => total + 1 + count(n.children as readonly { children: readonly unknown[] }[]),
        0,
      );
    })(built.roots);

    expect(placed + built.orphans.length + built.cyclic.length).toBe(teams.length);
  });
});
