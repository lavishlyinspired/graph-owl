/** Types mirror the Rust wire contract. Epic 1 Slice J generates these from
 *  OpenAPI; until then they are hand-written and this file is the one place
 *  that drifts, which is why it is small and separate.
 *
 *  Token management uses the auth module's in-memory storage — no tokens
 *  reach localStorage, sessionStorage, or cookies. */

export type AssetKind = "service" | "database" | "schema" | "table" | "column";

export interface EntityVersion {
  major: number;
  minor: number;
}

export interface FieldChange {
  field: string;
  before: unknown;
  after: unknown;
}

export interface ChangeDescription {
  fieldsAdded: FieldChange[];
  fieldsUpdated: FieldChange[];
  fieldsDeleted: FieldChange[];
}

export interface Asset {
  id: string;
  kind: AssetKind;
  name: string;
  fullyQualifiedName: string;
  parentId: string | null;
  description: string | null;
  properties?: Record<string, unknown> | null;
  version: EntityVersion;
  updatedBy: string;
  changeDescription?: ChangeDescription | null;
  deleted: boolean;
  deletedAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AssetVersion {
  version: EntityVersion;
  snapshot: Asset;
  changeDescription?: ChangeDescription | null;
  updatedBy: string;
  updatedAt: string;
}

/** One facet bucket. `count` is over the *visible* set — the server computes
 *  facets after authorization, so a bucket never reveals a schema the reader
 *  may not see, nor how big it is. */
export interface Facet {
  value: string;
  count: number;
}

export type { LineageGraph, LineageNode, LineageEdge } from "./graph/lineage";
export type { Finding, Severity, Suggestion } from "./governance/queue";
export type { Explanation } from "./governance/explanation";
export type { Solution } from "./workbench/results";

/** What a SPARQL query returned, and what it cost. */
export interface SparqlResult {
  readonly rows: readonly import("./workbench/results").Solution[];
  readonly factsScanned: number;
  /** The budget cut the answer short. **Always present**, never inferred from
   *  the row count — a truncated answer that looks complete is the failure
   *  this project refuses everywhere. */
  readonly truncated: boolean;
  readonly asOf: number | null;
  /** What the engine decided to read, one entry per scan. */
  readonly plan: readonly string[];
  /** The projected variables, **in the order the query named them**. Solutions
   *  arrive as sorted maps, so this is the only place that order survives. */
  readonly variables: readonly string[];
}

/** The stored violations queue, and the instant it reflects. */
export interface ValidationReport {
  readonly data: readonly import("./governance/queue").Finding[];
  /** The graph instant this reflects. `0` means no pass has ever run — which
   *  is a different statement from "nothing is wrong", and the only thing that
   *  makes an empty queue trustworthy. */
  readonly computedAtT: number;
  readonly total: number;
}

/** What one validation pass found. */
export interface ValidationRun {
  readonly conforms: boolean;
  readonly violations: number;
  readonly warnings: number;
  readonly info: number;
  readonly shapes: number;
  /** Shapes that could not be compiled. A pass over eighteen of twenty shapes
   *  produces a clean-looking report for the two that did not run. */
  readonly refusedShapes: number;
  readonly computedAtT: number;
}

/** What one reasoning run concluded. */
export interface ReasoningReport {
  readonly derived: number;
  readonly replaced: number;
  readonly iterations: number;
  readonly capped: string | null;
  readonly durationMs: number;
}
import type { LineageGraph } from "./graph/lineage";
import type { Explanation } from "./governance/explanation";

export interface ConnectorRun {
  id: string;
  connector: string;
  serviceName: string;
  startedAt: string;
  /** Null means the run never reported back — a crash, not a fast success. */
  finishedAt: string | null;
  created: number;
  skipped: number;
  failed: number;
  deleted: number;
  failures: unknown[];
  /** Why deletion detection declined. A refusal is a successful run that
   *  deliberately did nothing. */
  refusal: string | null;
  triggeredBy: string;
}

export interface SearchFacets {
  kind: Facet[];
  schema: Facet[];
}

export interface GraphNode {
  id: string;
  name: string;
  /** Null when the reader may not see the node. It stays in the picture as a
   *  bare node — removing it would claim a smaller neighbourhood than exists. */
  kind: AssetKind | null;
  fullyQualifiedName?: string;
}

export interface GraphEdge {
  from: string;
  to: string;
  relationship: string;
  /** The reasoner concluded this edge; nobody asserted it. Optional on the
   *  wire so an older server does not break the console — absent reads as
   *  asserted, which understates rather than overstates what was inferred. */
  derived?: boolean;
}

export interface GraphView {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** The walk hit its bound. Always shown — a partial picture presented as
   *  complete is the failure mode of every graph tool. */
  truncated: boolean;
}

export interface Overview {
  assets: { total: number; byKind: { kind: AssetKind; count: number }[] };
  documentation: { described: number; total: number };
  /** Null when no graph engine is configured — distinct from a graph of size
   *  zero, which is what a configured-but-empty projection looks like. */
  graph: { flakes: number } | null;
  recentlyChanged: Asset[];
}

export interface Page<T> {
  data: T[];
  paging: { after: string | null };
}

/** RFC 9457. Clients branch on `type`, never on prose — 01-api-conventions.md. */
export interface Problem {
  type: string;
  title: string;
  status: number;
  detail: string;
  errors?: { field: string; code: string; detail: string }[];
}

export class ApiError extends Error {
  constructor(readonly problem: Problem) {
    super(problem.detail);
  }
}

const BASE = import.meta.env.DEV ? "/api" : "";

/** Imported lazily to avoid circular dependency — auth module imports from
 *  this file, and the refresh function is only needed during 401 handling. */
let _tryRefresh: (() => Promise<boolean>) | null = null;

export function setRefreshHandler(fn: () => Promise<boolean>) {
  _tryRefresh = fn;
}

/** The stable `type` URI for an unauthenticated request. Branching on this
 *  rather than on `status` keeps the client honest about *why* it was refused:
 *  a 401 from a proxy and a 401 from graph-owl mean different things. */
const UNAUTHENTICATED = "https://graph-owl.dev/errors/unauthenticated";
const TOKEN_EXPIRED = "https://graph-owl.dev/errors/token-expired";
const TOKEN_INVALID = "https://graph-owl.dev/errors/token-invalid";
const FORBIDDEN = "https://graph-owl.dev/errors/forbidden";

export function isUnauthenticated(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === UNAUTHENTICATED;
}

export function isForbidden(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === FORBIDDEN;
}

export function isTokenExpired(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === TOKEN_EXPIRED;
}

export function isTokenInvalid(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === TOKEN_INVALID;
}

/** Where the bearer token is *read from*, not where it is kept.
 *
 *  **This module deliberately does not hold the token.** It held its own copy
 *  once, beside the one in `auth/`, and nothing assigned to it — so every
 *  request went out unauthenticated after a completely successful sign-in, and
 *  the console showed the sign-in screen to a user who had just signed in.
 *
 *  Two modules each believing they own "the" token is what produced that, and
 *  a setter here would only make the two copies syncable rather than singular.
 *  `auth/` obtains, refreshes and clears the token; this asks it for the
 *  current one on every request, so there is exactly one value and it cannot
 *  go stale.
 *
 *  The default returns `null` rather than throwing: a server in open mode has
 *  no token and no auth module wired, and that is a valid deployment. */
let _tokenSource: () => string | null = () => null;

export function setTokenSource(source: () => string | null) {
  _tokenSource = source;
}

export function authToken(): string | null {
  return _tokenSource();
}

async function request<T>(path: string, init?: RequestInit, retried?: boolean): Promise<T> {
  const token = authToken();
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const problem = (await response.json()) as Problem;

    // 401 with a refresh handler and no prior retry: try silent refresh once.
    if (problem.status === 401 && _tryRefresh && !retried) {
      const refreshed = await _tryRefresh();
      if (refreshed) return request<T>(path, init, true);
    }

    // A denied or failed request must never surface as an empty result —
    // that teaches the user the data does not exist. Callers are responsible
    // for honouring this; see `isUnauthenticated`.
    throw new ApiError(problem);
  }
  return (await response.json()) as T;
}

export const api = {
  roots: () => request<Asset[]>("/assets/roots"),
  /** `asOf` is an RFC 3339 instant. The server reconstructs the entity from
   *  the graph at that transaction time — it is not a snapshot lookup. */
  asset: (id: string, asOf?: string | null) =>
    request<Asset>(
      `/assets/${id}${asOf ? `?asOf=${encodeURIComponent(asOf)}` : ""}`,
    ),
  children: (id: string) => request<Asset[]>(`/assets/${id}/children`),
  ancestors: (id: string) => request<Asset[]>(`/assets/${id}/ancestors`),
  search: (q: string, kind?: AssetKind) =>
    request<Page<Asset> & { facets: SearchFacets }>(
      `/assets/search?q=${encodeURIComponent(q)}${kind ? `&kind=${kind}` : ""}&limit=50`,
    ),
  /** The neighbourhood around an asset. Labels are resolved server-side —
   *  one statement per traversal is pointless if the client then makes one
   *  request per node. */
  graph: (id: string, hops: number, asOf?: string | null) =>
    request<GraphView>(
      `/assets/${id}/graph?hops=${hops}` +
        (asOf ? `&asOf=${encodeURIComponent(asOf)}` : ""),
    ),
  /** One request for the whole landing page. Six would render in six stages
   *  and show a different partial truth in each. */
  overview: () => request<Overview>("/overview"),
  stats: () => request<{ byKind: { kind: AssetKind; count: number }[] }>("/assets/stats"),
  versions: (id: string) => request<AssetVersion[]>(`/assets/${id}/versions`),
  updateAsset: (id: string, update: { description: string | null }) =>
    request<Asset>(`/assets/${id}`, { method: "PATCH", body: JSON.stringify(update) }),
  runPostgresConnector: (body: {
    connectionString: string;
    serviceName: string;
    includeSchemas?: string[];
  }) =>
    request<{ runId: string; created: number; skipped: number; failed: number; failures: unknown[] }>(
      "/connectors/postgres/runs",
      { method: "POST", body: JSON.stringify(body) },
    ),
  /** Recent runs, newest first. A run that leaves a record nobody can see is
   *  only half the feature — "did last night's sync work" has to be answerable
   *  without a database session. */
  connectorRuns: () => request<ConnectorRun[]>("/connectors/runs?limit=20"),
  /** The lineage graph around an asset. Both directions, because "what breaks
   *  if I change this" and "where did this number come from" are the same graph
   *  read in opposite directions. */
  lineage: (id: string, upstream: number, downstream: number) =>
    request<LineageGraph>(
      `/lineage/asset/${id}?upstream=${upstream}&downstream=${downstream}`,
    ),
  /** The violations queue. Stored results, not a fresh pass — this is polled,
   *  and recomputing a full-graph validation per poll would make the cheapest
   *  client the most expensive query in the system. */
  validationReport: (params: { focusNode?: string; severity?: string } = {}) => {
    const query = new URLSearchParams();
    if (params.focusNode) query.set("focusNode", params.focusNode);
    if (params.severity) query.set("severity", params.severity);
    query.set("limit", "200");
    return request<ValidationReport>(`/validation/report?${query}`);
  },
  runValidation: () => request<ValidationRun>("/validation/runs", { method: "POST" }),
  /** Accept a violation. Reason and expiry are both required and the server
   *  refuses without them — a waiver nobody can review is a violation deleted
   *  with extra steps. */
  waiveFinding: (body: {
    shape: string;
    focusNode: string;
    path: string | null;
    constraint: string;
    reason: string;
    expiresAt: string;
  }) =>
    request<{ id: string }>("/validation/waivers", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
  revokeWaiver: (id: string) =>
    request<void>(`/validation/waivers/${id}`, { method: "DELETE" }),
  runReasoning: () => request<ReasoningReport>("/reasoning/runs", { method: "POST" }),
  /** Run a SPARQL query. The budget is the server's, not ours — a client that
   *  could raise its own limit does not have one. */
  sparql: (query: string) =>
    request<SparqlResult>("/sparql", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ query }),
    }),
  /** What the reasoner concluded about one subject, as the last run stored it.
   *  Not a fresh pass — an asset page opens with this. */
  derivedAbout: (subject: string) =>
    request<{ s: string; p: string; o: string; t: number }[]>(
      `/reasoning/derived?subject=${encodeURIComponent(subject)}`,
    ),
  /** Why a fact holds, all the way down to the assertions under it. */
  explain: (s: string, p: string, o: string) =>
    request<Explanation>(
      `/reasoning/explain?s=${encodeURIComponent(s)}&p=${encodeURIComponent(p)}&o=${encodeURIComponent(o)}`,
    ),
};
