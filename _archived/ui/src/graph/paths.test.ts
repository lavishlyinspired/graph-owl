import { describe, expect, it } from "vitest";
import { describeAnswer, nodeLabel, whyNotRunnable } from "./paths";
import type { PathAnswer } from "../api";

const answer = (overrides?: Partial<PathAnswer>): PathAnswer => ({
  paths: [{ nodes: ["1:a", "1:b", "1:c"], length: 2 }],
  truncated: false,
  ...overrides,
});

describe("deciding whether a connection question can be asked", () => {
  it("refuses until both ends are chosen, and says which is missing", () => {
    expect(whyNotRunnable({ from: "", to: "1:b" })).toContain("start");
    expect(whyNotRunnable({ from: "1:a", to: "" })).toContain("end");
  });

  /** **A node is trivially connected to itself, and saying so is not an
   *  answer.** Running the query would return a zero-length path and read as
   *  a successful result, which is worse than refusing. */
  it("refuses the same node at both ends", () => {
    expect(whyNotRunnable({ from: "1:a", to: "1:a" })).toContain("same");
  });

  it("allows two distinct nodes", () => {
    expect(whyNotRunnable({ from: "1:a", to: "1:b" })).toBeNull();
  });
});

describe("describing what came back", () => {
  /** **"Not connected" must be phrased as a bounded claim.** The server
   *  searched to a depth; saying "these are not connected" without saying how
   *  far it looked overstates what was actually established. */
  it("an empty answer names the depth that was searched", () => {
    const text = describeAnswer(answer({ paths: [] }), { hops: 4 });
    expect(text).toContain("4");
    expect(text.toLowerCase()).toContain("no route");
  });

  it("one route is reported as one route", () => {
    expect(describeAnswer(answer(), { hops: 4 })).toContain("1 route");
  });

  it("several routes are counted", () => {
    const two = answer({
      paths: [
        { nodes: ["1:a", "1:b", "1:d"], length: 2 },
        { nodes: ["1:a", "1:c", "1:d"], length: 2 },
      ],
    });
    expect(describeAnswer(two, { hops: 4 })).toContain("2 routes");
  });

  /** **A truncated answer must not read as a complete one.** The reader's
   *  conclusion from "1 route" is *there is one way these are connected* —
   *  a stronger claim than the server made. */
  it("a truncated answer says there are more", () => {
    const text = describeAnswer(answer({ truncated: true }), { hops: 4 });
    expect(text).toContain("at least");
    expect(text).not.toContain("no route");
  });
});

describe("labelling a node", () => {
  /** The console holds names for assets it has loaded and nothing for a node
   *  an import landed. Showing the raw identifier is honest; inventing a
   *  friendly label for it would not be. */
  it("prefers a known name and falls back to the identifier", () => {
    const known = new Map([["1:a", "warehouse.public.orders"]]);
    expect(nodeLabel("1:a", known)).toBe("warehouse.public.orders");
    expect(nodeLabel("1:zzz", known)).toBe("1:zzz");
  });

  /** A UUID-shaped local name is unreadable at a glance and identical to its
   *  neighbours for the first eight characters of nothing in particular — but
   *  it is still the only true identity, so it is shortened, never replaced. */
  it("shortens a long identifier without losing which node it is", () => {
    const long = "1:0f3a9c22-7b41-4e6a-9a0d-2b8c5d1e77aa";
    const shown = nodeLabel(long, new Map());
    expect(shown.length).toBeLessThan(long.length);
    expect(long).toContain(shown.replace("…", "").split(":")[1]!.slice(0, 8));
  });
});
