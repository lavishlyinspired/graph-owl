/** Types mirror the Rust wire contract. Epic 1 Slice J generates these from
 *  OpenAPI; until then they are hand-written and this file is the one place
 *  that drifts, which is why it is small and separate. */

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

/** The stable `type` URI for an unauthenticated request. Branching on this
 *  rather than on `status` keeps the client honest about *why* it was refused:
 *  a 401 from a proxy and a 401 from graph-owl mean different things. */
const UNAUTHENTICATED = "https://graph-owl.dev/errors/unauthenticated";

export function isUnauthenticated(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === UNAUTHENTICATED;
}

/** Where the bearer token lives.
 *
 *  `sessionStorage`, not `localStorage`: it is scoped to this tab and dies
 *  with it, so a token cannot outlive the session that pasted it or leak into
 *  another tab. `00f` says tokens live in memory only, and this is one step
 *  weaker than that — chosen deliberately, because memory-only means a page
 *  refresh silently logs you out, and a console that appears to forget you at
 *  random is the kind of thing people work around by writing the token down.
 *
 *  This is a stopgap for the manual token a demo pastes. When Epic 12's
 *  OIDC/PKCE lands, the token comes from the flow and this goes away. */
const TOKEN_KEY = "graphowl.token";

export function authToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function setAuthToken(token: string | null) {
  if (token === null) sessionStorage.removeItem(TOKEN_KEY);
  else sessionStorage.setItem(TOKEN_KEY, token);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
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
    // A denied or failed request must never surface as an empty result —
    // that teaches the user the data does not exist. Callers are responsible
    // for honouring this; see `isUnauthenticated`.
    throw new ApiError((await response.json()) as Problem);
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
    request<{ created: number; failed: number; failures: unknown[] }>(
      "/connectors/postgres/runs",
      { method: "POST", body: JSON.stringify(body) },
    ),
};
