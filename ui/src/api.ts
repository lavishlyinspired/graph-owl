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

export type OwnerKind = "user" | "team";

/** A denormalized owner reference — Epic 11 Slices C and D. The server
 *  resolves `displayName` at read time, so a renamed team shows correctly
 *  here without a follow-up request. */
export interface EntityReference {
  id: string;
  kind: OwnerKind;
  displayName: string;
  /** Found by walking up the containment hierarchy rather than recorded on
   *  this entity itself. Always present — an older server that predates
   *  inheritance is not the same fact as "recorded directly", and collapsing
   *  the two would make a console read a 5,000-table catalog as fully owned
   *  when nobody has named an owner below the database. */
  inherited: boolean;
}

/** A team, as `GET /teams` returns it — Epic 11 Slices B and C. */
export interface Team {
  id: string;
  displayName: string;
  description: string | null;
  members: string[];
  /** The team this one reports into, `null` for a root. Always present: a console
   *  reading its absence cannot tell "top of the hierarchy" from "a server that
   *  does not know about nesting". */
  parentTeamId: string | null;
}

export interface Asset {
  id: string;
  kind: AssetKind;
  name: string;
  fullyQualifiedName: string;
  parentId: string | null;
  description: string | null;
  properties?: Record<string, unknown> | null;
  /** `[]` when unowned — an asset with no owner is a real, reportable state,
   *  not an absent field.
   *
   *  **Optional here even though the server always sends it.** A required type
   *  is a claim about the build, not about whichever server is answering: when
   *  this was declared required, an older server omitting the field took the
   *  whole asset page down. The optionality is what forces every reader to
   *  decide what an absent field means rather than assume `[]`. */
  owners?: EntityReference[];
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
  /** `SERVICE` endpoints that answered — Epic 101. Result-level, not
   *  per-row: `spareval` gives no hook to attribute one bound row to the
   *  call that produced it, only the query as a whole. */
  readonly federatedEndpoints: readonly string[];
  /** `SERVICE SILENT` endpoints that could not be reached. **Always
   *  present, never inferred from row count** — a silently-failed clause
   *  contributes no error and often no rows either, which is otherwise
   *  indistinguishable from "this endpoint genuinely has no matching data". */
  readonly silencedFailures: readonly string[];
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
   *  zero, which is what a configured-but-empty projection looks like.
   *  `nodes` is comparable to `assets.total`: trailing it means the graph
   *  view is behind the entity store. */
  graph: { flakes: number; nodes: number; edges: number } | null;
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

/** What this server will accept as a credential, read before anything else.
 *
 *  **Deliberately not routed through `request`.** This is the call that decides
 *  whether a credential is needed at all, so it must not be able to trigger the
 *  401-and-refresh path it is trying to configure — and against a server old
 *  enough to lack the endpoint it 404s with a body that is not problem+json,
 *  which `request` would fail to parse rather than report.
 *
 *  Rejects rather than returning a default. The caller owns the fallback,
 *  because "the server did not answer" and "the server said open" must not
 *  arrive at the same branch. */
export async function fetchAuthConfig(): Promise<unknown> {
  const response = await fetch(`${BASE}/auth/config`);
  if (!response.ok) throw new Error(`auth configuration unavailable: ${response.status}`);
  return (await response.json()) as unknown;
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
  // `204 No Content` carries no body — `delete_team`/`delete_policy` and
  // every other idempotent-delete endpoint return it. Calling `.json()` on
  // an empty body throws `Unexpected end of JSON input`, which every caller
  // typed `request<void>()` was silently exposed to.
  if (response.status === 204) return undefined as T;
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
  // ---- Admin: principals and teams (Epic 41 Slice F over Epic 11) ----
  teams: () => request<Team[]>("/teams"),
  upsertTeam: (body: {
    id: string;
    displayName: string;
    description?: string | null;
    members: string[];
    parentTeamId?: string | null;
  }) => request<Team>("/teams", { method: "POST", body: JSON.stringify(body) }),
  childTeams: (id: string) => request<Team[]>(`/teams/${encodeURIComponent(id)}/children`),
  /** `reassignToKind` is required alongside `reassignTo`: a user and a team can
   *  share an id, and guessing would transfer an estate to the wrong principal. */
  deletePrincipal: (
    kind: OwnerKind,
    id: string,
    reassign?: { to: string; kind: OwnerKind },
  ) => {
    const collection = kind === "team" ? "teams" : "users";
    const query = reassign
      ? `?reassignTo=${encodeURIComponent(reassign.to)}&reassignToKind=${reassign.kind}`
      : "";
    return request<void>(`/${collection}/${encodeURIComponent(id)}${query}`, {
      method: "DELETE",
    });
  },
  upsertUser: (id: string, body: { displayName: string; email?: string | null }) =>
    request<{ id: string; displayName: string; email: string | null; roles: string[] }>(
      `/users/${encodeURIComponent(id)}`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  /** Try a connection **before** saving it. A test that could only run against a
   *  stored config would confirm the credential after the mistake was made. */
  testConnector: (connector: string, body: { settings: Record<string, unknown>; secret?: string }) =>
    request<{ ok: boolean; detail?: string }>(
      `/connectors/${encodeURIComponent(connector)}/test`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  /** Simulate a policy against a set of roles before it is saved. */
  dryRunPolicy: (policy: unknown, roles: string[]) =>
    request<import("./admin/policy").DryRun>("/policies/dry-run", {
      method: "POST",
      body: JSON.stringify({ policy, roles }),
    }),
  /** Every stored policy, with the roles it currently applies to. */
  policies: () =>
    request<{ policy: import("./admin/policy").Policy; roles: string[] }[]>("/policies"),
  /** Create or update a policy. `roles` replaces whatever it applied to
   *  before, not an addition to it. */
  savePolicy: (policy: unknown, roles: string[]) =>
    request<{ policy: import("./admin/policy").Policy; roles: string[] }>("/policies", {
      method: "POST",
      body: JSON.stringify({ policy, roles }),
    }),
  deletePolicy: (name: string) =>
    request<void>(`/policies/${encodeURIComponent(name)}`, { method: "DELETE" }),
  /** The `SERVICE` allow-list as currently configured. Read-only — Epic 101
   *  Slice E — there is nothing to write to yet; see the server route's own
   *  doc comment. */
  federationEndpoints: () => request<{ endpoints: string[] }>("/admin/federation"),
  /** What we know about a subject, best first, each with its staleness and
   *  score decomposition. Entity-scoped — the Knowledge tab's own read. */
  recallMemories: (subjectId: string, query = "", includeSuperseded = false) =>
    request<import("./memory/memory").RecalledMemory[]>(
      `/assets/${encodeURIComponent(subjectId)}/memories?q=${encodeURIComponent(query)}` +
        (includeSuperseded ? "&includeSuperseded=true" : ""),
    ),
  createMemory: (body: {
    kind: import("./memory/memory").Memory["kind"];
    content: string;
    summary?: string | null;
    confidence?: number;
    links: import("./memory/memory").MemoryLink[];
  }) => request<import("./memory/memory").Memory>("/memories", { method: "POST", body: JSON.stringify(body) }),
  supersedeMemory: (
    id: string,
    body: {
      kind: import("./memory/memory").Memory["kind"];
      content: string;
      summary?: string | null;
      confidence?: number;
      links: import("./memory/memory").MemoryLink[];
    },
  ) =>
    request<import("./memory/memory").Memory>(`/memories/${encodeURIComponent(id)}/supersede`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  retractMemory: (id: string, reason: string) =>
    request<import("./memory/memory").Memory>(`/memories/${encodeURIComponent(id)}/retract`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  /** Cross-entity search over every memory, for administration. Retracted
   *  memories are excluded unless asked for — the working view should not
   *  be cluttered with what nobody believes any more. */
  searchMemories: (filter: {
    author?: string;
    minConfidence?: number;
    maxConfidence?: number;
    since?: string;
    until?: string;
    includeRetracted?: boolean;
  }) => {
    const params = new URLSearchParams();
    if (filter.author) params.set("author", filter.author);
    if (filter.minConfidence !== undefined) params.set("minConfidence", String(filter.minConfidence));
    if (filter.maxConfidence !== undefined) params.set("maxConfidence", String(filter.maxConfidence));
    if (filter.since) params.set("since", filter.since);
    if (filter.until) params.set("until", filter.until);
    if (filter.includeRetracted) params.set("includeRetracted", "true");
    return request<{ data: import("./memory/memory").Memory[]; total: number }>(
      `/memories?${params.toString()}`,
    );
  },
  /** What a connector needs configured, as its own JSON Schema — so a hundred
   *  connectors do not become a hundred hand-written forms. */
  connectorSchema: (connector: string) =>
    request<Record<string, unknown>>(`/connectors/${encodeURIComponent(connector)}/schema`),

  /** Why a fact holds, all the way down to the assertions under it. */
  explain: (s: string, p: string, o: string) =>
    request<Explanation>(
      `/reasoning/explain?s=${encodeURIComponent(s)}&p=${encodeURIComponent(p)}&o=${encodeURIComponent(o)}`,
    ),
};
