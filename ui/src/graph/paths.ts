/** Everything decidable about a connection question — Plan 111 Slice A.
 *
 *  The panel that uses this is an imperative shell: pick two nodes, call the
 *  route, draw the answer. Every judgement it makes lives here so it can be
 *  asserted directly, which is the same split `graph/cytoscape.ts` already
 *  draws against `GraphCanvas`.
 *
 *  **Nothing here names a domain.** A node is an identifier, a route is a
 *  list of identifiers, and the prose below describes shapes rather than
 *  subjects — GST reads a route as *invoice → supplier → filing*, a
 *  healthcare pack as *patient → encounter → medication*, and this module
 *  cannot tell which it is looking at. */

import type { PathAnswer } from "../api";

/** Why this question cannot be asked yet, or `null` if it can. */
export function whyNotRunnable({ from, to }: { from: string; to: string }): string | null {
  if (from.trim() === "") return "Choose a start node.";
  if (to.trim() === "") return "Choose an end node.";
  // A node is trivially connected to itself. Running the query would return a
  // zero-length route and read as a successful answer, which is worse than a
  // refusal because it looks like it established something.
  if (from.trim() === to.trim()) return "Start and end are the same node.";
  return null;
}

/** One sentence stating exactly what was found, and how hard it looked.
 *
 *  **Every phrasing here is bounded on purpose.** "These are not connected"
 *  is a claim about the whole graph; what the server established is "no route
 *  within N hops", and the difference is the difference between a fact and an
 *  overstatement. */
export function describeAnswer(answer: PathAnswer, { hops }: { hops: number }): string {
  const count = answer.paths.length;
  if (count === 0) {
    return `No route within ${hops} hops.`;
  }
  const noun = count === 1 ? "route" : "routes";
  // A capped enumeration must never read as an exhaustive one: the reader's
  // conclusion from "1 route" is *there is one way these are connected*.
  const qualifier = answer.truncated ? "at least " : "";
  return `${qualifier}${count} ${noun} within ${hops} hops.`;
}

/** How long an identifier may run before it is shortened. Chosen to fit the
 *  longest thing worth reading whole — a `namespace:` prefix plus a short
 *  local name — rather than to fit a particular screen. */
const READABLE = 20;

/** What to show for a node.
 *
 *  A known name when the console has loaded one; otherwise the identifier
 *  itself, shortened but never replaced. **An invented friendly label for a
 *  node nothing has named would be a guess presented as a fact**, and the
 *  identifier is the only thing here that is certainly true. */
export function nodeLabel(id: string, names: ReadonlyMap<string, string>): string {
  const known = names.get(id);
  if (known !== undefined && known !== "") return known;
  if (id.length <= READABLE) return id;
  return `${id.slice(0, READABLE)}…`;
}
