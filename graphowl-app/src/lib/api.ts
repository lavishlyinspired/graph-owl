/** Hand-written wrapper over the generated `api.d.ts`, same split
 *  `ui/src/api.ts` uses: the generated file is regenerated from
 *  `openapi.json` and never hand-edited; endpoints without a registered
 *  response schema (`/inbox`, `/search` — see `openapi.rs`'s `ROUTES`
 *  table) get their shape declared here instead, matching exactly what
 *  `crates/graph-owl-server/src/lib.rs`'s handlers actually return. */

import { resolveInboxAction, type InboxAction } from "./inboxActions";

const API_BASE = "";

async function apiFetch<T>(path: string): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: { accept: "application/json" },
  });
  if (!response.ok) {
    throw new Error(`${path} responded ${response.status}`);
  }
  return (await response.json()) as T;
}

export interface InboxItem {
  readonly source: string;
  readonly id: string;
  readonly tag: string;
  readonly title: string;
  readonly detail: string;
  readonly who: string | null;
  readonly createdAt?: string;
}

export interface InboxCounts {
  readonly agentProposals: number;
  readonly changeProposals: number;
  readonly resolutionQueue: number;
  readonly findings: number;
  readonly extractionClaims: number;
}

export interface InboxResponse {
  readonly items: readonly InboxItem[];
  readonly counts: InboxCounts;
}

export function fetchInbox(): Promise<InboxResponse> {
  return apiFetch<InboxResponse>("/inbox");
}

export interface SearchResult {
  readonly kind: "asset" | "glossary-term" | "business-metric";
  readonly id: string;
  readonly label: string;
  readonly fqn: string;
  readonly detail: string | null;
  readonly assetKind?: string;
}

export function search(query: string): Promise<SearchResult[]> {
  if (query.trim().length === 0) return Promise.resolve([]);
  return apiFetch<SearchResult[]>(`/search?q=${encodeURIComponent(query)}`);
}

export async function performInboxAction(action: InboxAction): Promise<void> {
  const response = await fetch(`${API_BASE}${action.path}`, {
    method: action.method,
    headers: action.body ? { "content-type": "application/json" } : undefined,
    body: action.body ? JSON.stringify(action.body) : undefined,
  });
  if (!response.ok) {
    throw new Error(`${action.path} responded ${response.status}`);
  }
}

// Re-exported so callers only need one import for both the action mapping
// and the fetch that executes it.
export { resolveInboxAction };
export type { InboxAction };
