/** Everything decidable about narrowing a neighbourhood to certain edge
 *  names — Plan 112 Slice A.
 *
 *  `EdgeFilter.relationship_types` has existed in the traversal engine since
 *  Epic 7a and `asset_subgraph` passed `None` for it unconditionally, so the
 *  explorer had depth and time and neither of the other two controls the
 *  capability assessment asks for. A hub node's neighbourhood is unreadable
 *  without this: the picture is complete and tells the reader nothing.
 *
 *  **Nothing here knows any edge name.** The vocabulary comes from the walk —
 *  a catalog edge is one of `RelationshipType`'s names, a pack's edge is
 *  whatever its own `relType` flakes hold — so a hospitality deployment gets
 *  its own list with no change here, and
 *  `scripts/check-namespace-neutrality.py` fails the build if a name ever
 *  appears in this file. */

/** Every edge name the reader has been offered so far, plus whatever this
 *  walk returned.
 *
 *  **Accumulated rather than replaced, and that is the whole point.** Deriving
 *  the options from the current response is circular: a filtered response
 *  contains only the selected kinds, so the option list would collapse to the
 *  selection and the reader could never widen it again. Sorted so the control
 *  does not reorder under them between walks. */
export function knownKinds(
  seen: Iterable<string>,
  fromThisWalk: Iterable<string>,
): readonly string[] {
  return [...new Set([...seen, ...fromThisWalk])].sort((a, b) => a.localeCompare(b));
}

/** Every edge name in the **unfiltered** neighbourhood, from the analytics
 *  payload.
 *
 *  **This is the source `knownKinds` cannot be.** Accumulating names out of
 *  walk responses only works when an unfiltered walk happened first — and a
 *  *shared* filtered URL (`?edges=...`) is loaded with the filter already set,
 *  so nothing ever accumulates: the control disappears and an empty picture
 *  gets blamed on the depth rather than on the filter. Found in a browser, on
 *  the very link this slice made shareable.
 *
 *  Analytics walks unfiltered by construction, so its `edgeTypes` answers
 *  "every edge name here" no matter what the current walk was narrowed to.
 *  The names arrive as rendered `Sid`s (`1:parentSchema`) and the graph's own
 *  edges are bare (`parentSchema`); comparing the two forms without stripping
 *  would make every option fail to match the edges it filters. */
export function kindsFromAnalytics(edgeTypes: readonly string[]): readonly string[] {
  return [...new Set(edgeTypes.map((type) => type.slice(type.indexOf(":") + 1)))].sort((a, b) =>
    a.localeCompare(b),
  );
}

/** What to send as the request's filter, or `undefined` for no filter.
 *
 *  **No selection means no filter, and the decision lives here rather than on
 *  the server.** The server reads an explicitly empty list as *match nothing*,
 *  which is the honest reading of what an empty list says — so it is the
 *  console's job to send nothing at all when the reader has chosen nothing,
 *  or an untouched control would empty the graph on first load.
 *
 *  A selection naming kinds that no longer appear is passed through unchanged:
 *  the reader asked for them, and quietly widening the request would show a
 *  graph broader than the control claims. */
export function filterParam(selected: readonly string[]): readonly string[] | undefined {
  return selected.length === 0 ? undefined : selected;
}

/** Why the picture is empty, or `null` when it is not.
 *
 *  **"Nothing matches these filters" and "nothing is connected" are different
 *  claims.** The second is a statement about the graph; showing it when a
 *  filter is responsible is the failure mode this slice is most likely to
 *  ship, and it sends the reader looking for missing data that is right there.
 *
 *  A filter is only blamed when the node genuinely has edges that the filter
 *  is hiding — otherwise the reader would be sent to adjust a control that
 *  cannot help. */
export function whyEmpty({
  selected,
  hasAnyEdge,
  edgesShown = 0,
}: {
  selected: readonly string[];
  /** Whether this node has any edge at all, from the unfiltered walk. */
  hasAnyEdge: boolean;
  edgesShown?: number;
}): string | null {
  if (edgesShown > 0) return null;
  if (selected.length > 0 && hasAnyEdge) {
    return "Nothing matches these filters. This node is connected — just not by the relationships you have selected.";
  }
  return "Nothing is connected to this node at this depth.";
}
