import { describe, expect, it } from "vitest";
import { filterParam, kindsFromAnalytics, knownKinds, whyEmpty } from "./edgeFilter";

describe("the relationship-type options a reader can choose from", () => {
  /** **The options must come from the unfiltered walk.** Deriving them from
   *  the *current* response is circular: once a filter is applied the response
   *  contains only the selected kinds, the option list collapses to the
   *  selection, and the reader can never widen it again. */
  it("remembers every kind ever seen, so filtering cannot shrink the choices", () => {
    const afterFirstWalk = knownKinds([], ["contains", "parentSchema"]);
    expect([...afterFirstWalk]).toEqual(["contains", "parentSchema"]);

    // A filtered walk returns only one kind. The option list must not narrow.
    const afterFiltering = knownKinds(afterFirstWalk, ["contains"]);
    expect([...afterFiltering]).toEqual(["contains", "parentSchema"]);
  });

  it("adds a kind an expansion discovers", () => {
    expect([...knownKinds(["contains"], ["contains", "derivedFrom"])]).toEqual([
      "contains",
      "derivedFrom",
    ]);
  });

  it("sorts, so the control does not reorder under the reader between walks", () => {
    expect([...knownKinds([], ["zeta", "alpha"])]).toEqual(["alpha", "zeta"]);
  });
});

describe("turning a selection into a request", () => {
  /** **No selection means no filter, and that decision lives here.** The
   *  server reads an explicitly empty list as "match nothing", which is the
   *  honest reading of what it says — so the console must send *nothing* when
   *  the reader has selected nothing, or an untouched control would empty the
   *  graph. */
  it("sends no filter at all when nothing is selected", () => {
    expect(filterParam([])).toBeUndefined();
  });

  it("sends the selection when there is one", () => {
    expect(filterParam(["contains"])).toEqual(["contains"]);
    expect(filterParam(["contains", "derivedFrom"])).toEqual(["contains", "derivedFrom"]);
  });

  /** A selection of kinds that no longer exist is still sent: the reader asked
   *  for them, and silently dropping the request would show a wider graph than
   *  the control claims. The server answers "nothing matches", which is true. */
  it("does not second-guess a selection against what is currently known", () => {
    expect(filterParam(["goneAway"])).toEqual(["goneAway"]);
  });
});

describe("explaining an empty picture", () => {
  /** **"Nothing matches these filters" and "nothing is connected" are
   *  different claims**, and showing the second when the first is true is the
   *  failure this slice is most likely to ship. */
  it("blames the filter when one is active", () => {
    const text = whyEmpty({ selected: ["contains"], hasAnyEdge: true });
    expect(text).not.toBeNull();
    expect(text!.toLowerCase()).toContain("filter");
  });

  it("says the node is unconnected only when no filter is active", () => {
    const text = whyEmpty({ selected: [], hasAnyEdge: false });
    expect(text).not.toBeNull();
    expect(text!.toLowerCase()).not.toContain("filter");
  });

  /** Nothing to explain when there is something on screen. */
  it("says nothing when the picture is not empty", () => {
    expect(whyEmpty({ selected: ["contains"], hasAnyEdge: true, edgesShown: 3 })).toBeNull();
    expect(whyEmpty({ selected: [], hasAnyEdge: true, edgesShown: 1 })).toBeNull();
  });

  /** A filter is active *and* the node has no edges at all — the filter is not
   *  the reason, so blaming it would send the reader to adjust a control that
   *  cannot help. */
  it("does not blame the filter when the node has no edges at all", () => {
    const text = whyEmpty({ selected: ["contains"], hasAnyEdge: false });
    expect(text!.toLowerCase()).not.toContain("filter");
  });

  /** **No filter, edges known, none shown at this depth** — reachable because
   *  `hasAnyEdge` accumulates across walks, so reducing the depth can leave a
   *  node whose edge names are known but whose current walk shows none. The
   *  honest answer is the depth, not a filter that is not set. Written to kill
   *  a mutant that dropped the `selected.length > 0` guard and blamed the
   *  filter regardless. */
  it("blames the depth, not an unset filter, when nothing shows at this depth", () => {
    const text = whyEmpty({ selected: [], hasAnyEdge: true, edgesShown: 0 });
    expect(text!.toLowerCase()).not.toContain("filter");
    expect(text!.toLowerCase()).toContain("depth");
  });

  /** And the two messages are genuinely different strings — a mutant that
   *  returned the filter message from both branches would otherwise pass every
   *  assertion above that only checks for the absence of the word. */
  it("says two different things in the two cases", () => {
    expect(whyEmpty({ selected: ["contains"], hasAnyEdge: true })).not.toBe(
      whyEmpty({ selected: [], hasAnyEdge: false }),
    );
  });
});

/** Plan 112 Slice B — the path finder sends the filter its own API already
 *  accepts. Same two functions, because the decision is identical: no
 *  selection means no filter, and the server reads an empty list as "match
 *  nothing". */
describe("the path finder's filter", () => {
  /** The mutant this exists for: the filter dropping to `undefined` on the way
   *  into the request, which reads as a working control that quietly follows
   *  every edge. */
  it("passes a selection straight through to the request", () => {
    expect(filterParam(["derivedFrom"])).toEqual(["derivedFrom"]);
  });

  it("sends nothing when the reader has selected nothing", () => {
    expect(filterParam([])).toBeUndefined();
  });
});

/** **Found in a browser, not in a test.** Opening a *shared* filtered URL —
 *  `?edges=parentDatabase` — means no unfiltered walk has ever happened in
 *  that session, so an accumulator seeded from walk responses is empty: the
 *  filter control disappears and the empty picture is blamed on the depth
 *  instead of on the filter. Both are wrong, and the second is the exact
 *  claim this slice exists to avoid making.
 *
 *  The fix is the source. Analytics computes over the **unfiltered**
 *  neighbourhood, so its `edgeTypes` is "every edge name here" regardless of
 *  what the current walk was narrowed to. */
describe("edge names that do not depend on having walked unfiltered first", () => {
  it("reads the names out of the analytics payload", () => {
    expect(kindsFromAnalytics(["1:parentSchema", "1:parentTable"])).toEqual([
      "parentSchema",
      "parentTable",
    ]);
  });

  /** Analytics renders them as `Sid`s and the graph's edges are bare names.
   *  Comparing the two forms without stripping would make every option fail to
   *  match its own edges. */
  it("strips the namespace so the option matches the edge it filters", () => {
    expect(kindsFromAnalytics(["1024:issuedBy"])).toEqual(["issuedBy"]);
    expect(kindsFromAnalytics(["alreadyBare"])).toEqual(["alreadyBare"]);
  });

  it("is empty for a neighbourhood with no edges, so no control is offered", () => {
    expect(kindsFromAnalytics([])).toEqual([]);
  });

  it("sorts and deduplicates, like the accumulator it replaces", () => {
    expect(kindsFromAnalytics(["1:zeta", "2:alpha", "3:zeta"])).toEqual(["alpha", "zeta"]);
  });
});
