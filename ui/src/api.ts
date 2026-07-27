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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  if (!response.ok) {
    // A denied or failed request must never surface as an empty result —
    // that teaches the user the data does not exist.
    throw new ApiError((await response.json()) as Problem);
  }
  return (await response.json()) as T;
}

export const api = {
  roots: () => request<Asset[]>("/assets/roots"),
  asset: (id: string) => request<Asset>(`/assets/${id}`),
  children: (id: string) => request<Asset[]>(`/assets/${id}/children`),
  ancestors: (id: string) => request<Asset[]>(`/assets/${id}/ancestors`),
  search: (q: string, kind?: AssetKind) =>
    request<Page<Asset>>(
      `/assets/search?q=${encodeURIComponent(q)}${kind ? `&kind=${kind}` : ""}&limit=50`,
    ),
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
