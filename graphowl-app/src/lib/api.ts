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

export interface OverviewHealth {
  readonly coveragePct: number;
  readonly governancePct: number;
}

export interface GraphSize {
  readonly flakes: number;
  readonly nodes: number;
  readonly edges: number;
}

export interface RecentlyChangedAsset {
  readonly id: string;
  readonly kind: string;
  readonly name: string;
  readonly fullyQualifiedName: string;
  readonly updatedAt: string;
  readonly updatedBy: string;
}

export interface OverviewResponse {
  readonly assets: {
    readonly total: number;
    readonly byKind: readonly { readonly kind: string; readonly count: number }[];
  };
  readonly documentation: {
    readonly described: number;
    readonly total: number;
  };
  readonly graph: GraphSize | null;
  readonly recentlyChanged: readonly RecentlyChangedAsset[];
  readonly health: OverviewHealth;
}

export function fetchOverview(): Promise<OverviewResponse> {
  return apiFetch<OverviewResponse>("/overview");
}

// ---- Explore + Entity (Plan 122a A3) ----

async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`${path} responded ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function apiDelete(path: string): Promise<void> {
  const response = await fetch(`${API_BASE}${path}`, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(`${path} responded ${response.status}`);
  }
}

export type AssetKind = "service" | "database" | "schema" | "table" | "column";

export interface GraphNode {
  readonly id: string;
  readonly name: string;
  /** `null` when the reader may not see the node, or the id resolves to
   *  nothing this catalog knows — kept in the picture as a bare node rather
   *  than dropped, since removing it would claim a smaller neighbourhood
   *  than the graph actually has. */
  readonly kind: AssetKind | null;
  readonly fullyQualifiedName?: string;
  readonly semanticType?: string | null;
}

export interface GraphEdge {
  readonly from: string;
  readonly to: string;
  readonly relationship: string;
  /** The reasoner concluded this edge; nobody asserted it. Absent reads as
   *  asserted — understating rather than overstating what was inferred. */
  readonly derived?: boolean;
}

export interface GraphView {
  readonly nodes: readonly GraphNode[];
  readonly edges: readonly GraphEdge[];
  /** The walk hit its bound. Always surfaced — a partial picture presented
   *  as complete is the failure mode of every graph tool. */
  readonly truncated: boolean;
}

export function fetchAssetGraph(
  id: string,
  options: {
    readonly direction?: "outgoing" | "incoming" | "both";
    readonly hops?: number;
    readonly relationshipTypes?: readonly string[];
  } = {},
): Promise<GraphView> {
  const params = new URLSearchParams();
  if (options.direction) params.set("direction", options.direction);
  if (options.hops !== undefined) params.set("hops", String(options.hops));
  if (options.relationshipTypes && options.relationshipTypes.length > 0) {
    params.set("relationshipTypes", options.relationshipTypes.join(","));
  }
  const qs = params.toString();
  return apiFetch<GraphView>(`/assets/${encodeURIComponent(id)}/graph${qs ? `?${qs}` : ""}`);
}

export interface EntityVersion {
  readonly major: number;
  readonly minor: number;
}

export interface ChangeDescription {
  readonly summary: string;
  readonly reason?: string | null;
}

export interface Asset {
  readonly id: string;
  readonly kind: AssetKind;
  readonly name: string;
  readonly fullyQualifiedName: string;
  readonly parentId: string | null;
  readonly description: string | null;
  readonly properties?: Record<string, unknown> | null;
  readonly owners?: readonly { readonly id: string; readonly kind: string }[];
  readonly version: EntityVersion;
  readonly updatedBy: string;
  readonly changeDescription?: ChangeDescription | null;
  readonly deleted: boolean;
  readonly deletedAt?: string | null;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export function fetchAsset(id: string): Promise<Asset> {
  return apiFetch<Asset>(`/assets/${encodeURIComponent(id)}`);
}

export interface AssetVersion {
  readonly version: EntityVersion;
  readonly snapshot: Asset;
  readonly changeDescription?: ChangeDescription | null;
  readonly updatedBy: string;
  readonly updatedAt: string;
}

export function fetchAssetVersions(id: string): Promise<readonly AssetVersion[]> {
  return apiFetch<readonly AssetVersion[]>(`/assets/${encodeURIComponent(id)}/versions`);
}

// ---- Memory & contradictions (Epic 31 / Epic 41) ----
//
// **Nothing here resolves anything.** The engine never picks a winner
// between two disagreeing memories and never hides either side —
// confirming a pair leaves it in the queue, flagged, because software that
// adjudicates institutional disagreement ends the argument without
// settling it. `reviewContradiction`'s `verdict` is `confirmed` (yes, these
// disagree) or `dismissed` (no, they don't) — there is no "accept A" or
// "accept B", because the domain model has no winner field to set one in.

export type Authorship =
  | { readonly kind: "human"; readonly userId: string }
  | { readonly kind: "agent"; readonly agentId: string; readonly model: string };

export type LinkRelation = "about" | "affects" | "evidence" | "follows" | "contradicts" | "mentions";

export interface MemoryLink {
  readonly relation: LinkRelation;
  readonly target: string;
}

export interface Memory {
  readonly id: string;
  readonly kind: "rationale" | "incident" | "decision" | "caveat";
  readonly content: string;
  readonly summary: string | null;
  readonly authorship: Authorship;
  readonly confidence: number;
  readonly links: readonly MemoryLink[];
  readonly asOf: string;
  readonly supersedes: string | null;
  readonly supersededBy: string | null;
  readonly retractedAt: string | null;
  readonly retractionReason: string | null;
}

export interface RecalledMemory {
  readonly memory: Memory;
}

export function recallMemories(subjectId: string): Promise<readonly RecalledMemory[]> {
  return apiFetch<readonly RecalledMemory[]>(`/assets/${encodeURIComponent(subjectId)}/memories?q=`);
}

export function pinToInvestigation(assetId: string, content: string): Promise<Memory> {
  return apiPost<Memory>("/memories", {
    kind: "rationale",
    content,
    links: [{ relation: "about", target: assetId }],
  });
}

export type ContradictionKind = "confirmed" | "declared" | "candidate";

export interface Contradiction {
  readonly a: string;
  readonly b: string;
  readonly subject: string | null;
  readonly kind: ContradictionKind;
}

export function fetchContradictions(subjectId: string): Promise<readonly Contradiction[]> {
  return apiFetch<readonly Contradiction[]>(`/assets/${encodeURIComponent(subjectId)}/contradictions`);
}

export function reviewContradiction(body: {
  readonly a: string;
  readonly b: string;
  readonly verdict: "confirmed" | "dismissed";
  readonly note?: string | null;
}): Promise<void> {
  return apiPost<void>("/contradictions/reviews", { ...body, note: body.note?.trim() || null });
}

// ---- TRACE: Lineage, Paths, History, Evidence (Plan 122a A4) ----

export interface LineageNode {
  readonly id: string;
  readonly name: string;
  readonly kind: AssetKind | null;
  readonly fullyQualifiedName: string;
  readonly deleted: boolean;
}

export interface LineageEdge {
  readonly id: string;
  readonly fromAssetId: string;
  readonly toAssetId: string;
  readonly relationship: string;
  readonly source: "manual" | "connector" | "openlineage" | "agent";
  readonly query?: string | null;
  readonly description?: string | null;
  readonly createdAt: string;
  readonly createdBy: string;
}

export interface LineageGraph {
  readonly rootId: string;
  readonly nodes: readonly LineageNode[];
  readonly edges: readonly LineageEdge[];
  readonly truncated: boolean;
}

export function fetchLineage(
  id: string,
  options: { readonly upstream?: number; readonly downstream?: number } = {},
): Promise<LineageGraph> {
  const params = new URLSearchParams();
  if (options.upstream !== undefined) params.set("upstream", String(options.upstream));
  if (options.downstream !== undefined) params.set("downstream", String(options.downstream));
  const qs = params.toString();
  return apiFetch<LineageGraph>(`/lineage/asset/${encodeURIComponent(id)}${qs ? `?${qs}` : ""}`);
}

export interface FoundPath {
  /** Raw subject identifiers along the route — `/graph/paths` resolves no
   *  names, only identity, so the UI shows these as-is (monospace, linking
   *  through to Explore/Entity for a readable name) rather than paying for
   *  a resolution round trip per node on every path search. */
  readonly nodes: readonly string[];
  readonly length: number;
}

export interface PathSearchResult {
  readonly paths: readonly FoundPath[];
  readonly truncated: boolean;
}

export function findPaths(from: string, to: string): Promise<PathSearchResult> {
  return apiPost<PathSearchResult>("/graph/paths", { from, to, maxPaths: 10 });
}

export interface FindingEvidence {
  readonly subject: string;
  readonly predicate: string;
  readonly value: string;
  readonly var?: string | null;
}

export interface Finding {
  readonly id: string;
  readonly pack: string;
  readonly label: string;
  readonly subject: string;
  readonly summary: string;
  readonly governedBy: string;
  readonly evidence: readonly FindingEvidence[];
  readonly status: "pending" | "accepted" | "rejected";
  readonly detectedAt: string;
  readonly subjectLabel?: string | null;
}

export function fetchFindings(): Promise<readonly Finding[]> {
  return apiFetch<readonly Finding[]>("/findings");
}

// ---- GOVERN group — Validation, Resolution, Drift, Governance (Plan 122a A5) ----

export type Severity = "violation" | "warning" | "info";

export interface ValidationAssignment {
  readonly id: string;
  readonly assignee: string;
  readonly assignedBy: string;
  readonly assignedAt: string;
}

export interface ValidationWaiver {
  readonly id: string;
  readonly reason: string;
  readonly waivedBy: string;
  readonly waivedAt: string;
  readonly expiresAt: string;
  readonly expired: boolean;
}

export interface ValidationFinding {
  readonly id: string;
  readonly shape: string;
  readonly focusNode: string;
  readonly path: string | null;
  readonly constraint: string;
  readonly severity: Severity;
  readonly message: string;
  readonly actual: string | null;
  readonly suggestion: string | null;
  readonly assignment: ValidationAssignment | null;
  readonly waiver: ValidationWaiver | null;
}

export interface ValidationReport {
  readonly data: readonly ValidationFinding[];
  readonly computedAtT: number;
  readonly total: number;
  readonly limit: number;
  readonly offset: number;
}

export function fetchValidationReport(
  filters: { readonly severity?: Severity; readonly limit?: number } = {},
): Promise<ValidationReport> {
  const params = new URLSearchParams();
  if (filters.severity) params.set("severity", filters.severity);
  params.set("limit", String(filters.limit ?? 200));
  return apiFetch<ValidationReport>(`/validation/report?${params.toString()}`);
}

export interface WaiveFindingRequest {
  readonly shape: string;
  readonly focusNode: string;
  readonly path?: string | null;
  readonly constraint: string;
  readonly reason: string;
  readonly expiresAt: string;
}

export function waiveFinding(body: WaiveFindingRequest): Promise<ValidationWaiver> {
  return apiPost<ValidationWaiver>("/validation/waivers", body);
}

export function revokeWaiver(id: string): Promise<void> {
  return apiDelete(`/validation/waivers/${encodeURIComponent(id)}`);
}

export interface AssignFindingRequest {
  readonly shape: string;
  readonly focusNode: string;
  readonly path?: string | null;
  readonly constraint: string;
  readonly assignee: string;
}

export function assignFinding(body: AssignFindingRequest): Promise<ValidationAssignment> {
  return apiPost<ValidationAssignment>("/validation/assignments", body);
}

export function unassignFinding(id: string): Promise<void> {
  return apiDelete(`/validation/assignments/${encodeURIComponent(id)}`);
}

export type ReviewStatus = "pending" | "confirmed" | "rejected";

export type Evidence =
  | { readonly kind: "exactFqn" }
  | { readonly kind: "normalizedFqn" }
  | { readonly kind: "exactName"; readonly scope: string }
  | { readonly kind: "nameSimilarity"; readonly metric: string; readonly value: number }
  | { readonly kind: "structuralOverlap"; readonly sharedColumns: number; readonly total: number }
  | { readonly kind: "sameParent" }
  | { readonly kind: "sameSourceSystem" };

export interface ReviewQueueEntry {
  readonly id: string;
  readonly target: string;
  readonly candidate: string;
  readonly score: number;
  readonly evidence: readonly Evidence[];
  readonly status: ReviewStatus;
  readonly decidedBy: string | null;
  readonly decidedAt: string | null;
  readonly reason: string | null;
  readonly createdAt: string;
}

export interface ReviewQueuePage {
  readonly data: readonly ReviewQueueEntry[];
  readonly total: number;
}

export function fetchResolutionQueue(
  filters: { readonly status?: ReviewStatus; readonly limit?: number } = {},
): Promise<ReviewQueuePage> {
  const params = new URLSearchParams();
  if (filters.status) params.set("status", filters.status);
  params.set("limit", String(filters.limit ?? 200));
  return apiFetch<ReviewQueuePage>(`/resolution/queue?${params.toString()}`);
}

export type Resolution =
  | { readonly kind: "new" }
  | { readonly kind: "existing"; readonly entity: string; readonly confidence: number }
  | { readonly kind: "ambiguous"; readonly candidates: readonly unknown[] };

export function confirmReview(id: string): Promise<Resolution> {
  return apiPost<Resolution>(`/resolution/queue/${encodeURIComponent(id)}/confirm`, {});
}

export function rejectReview(id: string, reason: string): Promise<Resolution> {
  return apiPost<Resolution>(`/resolution/queue/${encodeURIComponent(id)}/reject`, { reason });
}

export interface BulkDecideOutcome {
  readonly id: string;
  readonly ok: boolean;
  readonly problem?: unknown;
}

export function bulkDecideReview(
  ids: readonly string[],
  decision: "confirm" | "reject",
  reason?: string,
): Promise<{ readonly data: readonly BulkDecideOutcome[] }> {
  return apiPost<{ readonly data: readonly BulkDecideOutcome[] }>("/resolution/queue/bulk", {
    ids,
    decision,
    reason,
  });
}

export type DriftKind = "liveEdited" | "unapplied";
export type DriftStatus = "pending" | "applied" | "ignored";

export interface DriftItem {
  readonly id: string;
  readonly assetId: string;
  readonly fullyQualifiedName: string;
  readonly field: string;
  readonly kind: DriftKind;
  readonly liveValue: string | null;
  readonly declaredValue: string | null;
  readonly status: DriftStatus;
  readonly reportedAt: string;
  readonly decidedAt: string | null;
  readonly decidedBy: string | null;
  readonly reason: string | null;
}

export interface DriftPage {
  readonly data: readonly DriftItem[];
  readonly total: number;
}

export function fetchDrift(
  filters: { readonly status?: DriftStatus; readonly limit?: number } = {},
): Promise<DriftPage> {
  const params = new URLSearchParams();
  if (filters.status) params.set("status", filters.status);
  params.set("limit", String(filters.limit ?? 200));
  return apiFetch<DriftPage>(`/drift?${params.toString()}`);
}

export function applyDrift(id: string): Promise<DriftItem> {
  return apiPost<DriftItem>(`/drift/${encodeURIComponent(id)}/apply`, {});
}

export function ignoreDrift(id: string, reason: string): Promise<DriftItem> {
  return apiPost<DriftItem>(`/drift/${encodeURIComponent(id)}/ignore`, { reason });
}

export type PolicyEffect = "allow" | "deny";

export type ResourceMatcher =
  | { readonly type: "all" }
  | { readonly type: "fqnPrefix"; readonly value: string }
  | { readonly type: "tagged"; readonly value: string }
  | { readonly type: "namedGraph"; readonly value: string };

export type MetadataOperation =
  | "viewBasic"
  | "viewDetails"
  | "viewSensitive"
  | "create"
  | "editDescription"
  | "editTags"
  | "editOwners"
  | "delete"
  | "restore";

export interface PolicyRule {
  readonly name: string;
  readonly effect: PolicyEffect;
  readonly operations: readonly MetadataOperation[];
  readonly resources: ResourceMatcher;
}

export interface Policy {
  readonly name: string;
  readonly rules: readonly PolicyRule[];
}

export interface PolicyBinding {
  readonly policy: Policy;
  readonly roles: readonly string[];
}

export function fetchPolicies(): Promise<readonly PolicyBinding[]> {
  return apiFetch<readonly PolicyBinding[]>("/policies");
}

export function upsertPolicy(policy: Policy, roles: readonly string[]): Promise<PolicyBinding> {
  return apiPost<PolicyBinding>("/policies", { policy, roles });
}

export function deletePolicy(name: string): Promise<void> {
  return apiDelete(`/policies/${encodeURIComponent(name)}`);
}

export interface DryRunOutcome {
  readonly admitted: number;
  readonly denied: number;
  readonly total: number;
  readonly examples: readonly string[];
  readonly admitsEverything: boolean;
}

export function dryRunPolicy(policy: Policy, roles: readonly string[]): Promise<DryRunOutcome> {
  return apiPost<DryRunOutcome>("/policies/dry-run", { policy, roles });
}

export type LifecycleState = "draft" | "active" | "deprecated" | "retired";

export interface AssetCount {
  readonly count: number;
  /** `true` when more rows exist past the fetched page — the count is a
   *  floor, not exact, the same honesty the Lineage "COMPLETE PICTURE" tile
   *  already uses for a cursor-paged result with no server-side total. */
  readonly truncated: boolean;
}

export async function countAssets(
  filter: { readonly unowned?: boolean; readonly lifecycle?: LifecycleState },
  sampleSize = 200,
): Promise<AssetCount> {
  const params = new URLSearchParams();
  if (filter.unowned) params.set("unowned", "true");
  if (filter.lifecycle) params.set("lifecycle", filter.lifecycle);
  params.set("limit", String(sampleSize));
  const page = await apiFetch<{ readonly data: readonly unknown[]; readonly paging: { readonly after: string | null } }>(
    `/assets?${params.toString()}`,
  );
  return { count: page.data.length, truncated: page.paging.after !== null };
}

export function fetchRecertificationQueue(): Promise<readonly unknown[]> {
  return apiFetch<readonly unknown[]>("/recertification-queue");
}
