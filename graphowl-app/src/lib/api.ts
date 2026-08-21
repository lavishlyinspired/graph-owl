/** Hand-written wrapper over the generated `api.d.ts`, same split
 *  `ui/src/api.ts` uses: the generated file is regenerated from
 *  `openapi.json` and never hand-edited; endpoints without a registered
 *  response schema (`/inbox`, `/search` — see `openapi.rs`'s `ROUTES`
 *  table) get their shape declared here instead, matching exactly what
 *  `crates/graph-owl-server/src/lib.rs`'s handlers actually return. */

import { resolveInboxAction, type InboxAction } from "./inboxActions";
import { findingsQueryString } from "./findingsQueue";
import {
  nodeTypeQuery,
  toGraphView,
  typesFromTypeRows,
  withNodeTypes,
  type RawGraphContext,
} from "./graph/graphContext";

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

// ---- "Ask GraphOWL" (`graphowl-app/plans/ask-graphowl.md`) — a separate,
// out-of-process agent (`examples/gst-reconcile/ask_server.py`), never
// graph-owl-server itself, per `plans/00j-language-boundaries.md`. Answers
// a fixed set of GST reconciliation questions by real deterministic
// findings, optionally narrated by whatever OpenAI-compatible model
// (Ollama included) the agent process is configured with. ----

export type AskResult =
  | { readonly kind: "noMatch"; readonly message: string }
  | { readonly kind: "error"; readonly message: string }
  | {
      readonly kind: "answered";
      readonly questionNumber: number;
      readonly answer: string;
      readonly narration?: string;
      readonly narrationError?: string;
    };

export function askGraphOwl(question: string): Promise<AskResult> {
  return apiPost<AskResult>("/ask", { question });
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

/** Every admin-gated route in this server refuses a non-admin caller with
 *  a bare `404` (never `403` — a `403` would confirm the route exists to
 *  somebody probing for it), so this is the one place that distinction is
 *  interpreted, reused by every admin-only panel rather than re-derived
 *  per component. */
export function isAdminOnlyError(error: unknown): boolean {
  return error instanceof Error && error.message.includes(" 404");
}

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

async function apiPut<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    method: "PUT",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`${path} responded ${response.status}`);
  }
  return (await response.json()) as T;
}

async function apiPatch<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    method: "PATCH",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`${path} responded ${response.status}`);
  }
  return (await response.json()) as T;
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
  /** The named import graphs this subject was seen in — real, checkable
   *  provenance, and the only provenance the graph API actually returns. */
  readonly sources?: readonly string[];
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

/** The neighbourhood of **any** graph subject, not only a catalog asset —
 *  counterpart to `fetchAssetGraph` for a seed that has no `assets` row at
 *  all (a finding's subject, most often). See `lib/graph/graphContext.ts`
 *  for why the response is remapped rather than used as-is. */
export function fetchGraphContext(
  seed: string,
  options: {
    readonly direction?: "outgoing" | "incoming" | "both";
    readonly hops?: number;
    readonly relationshipTypes?: readonly string[];
  } = {},
): Promise<GraphView> {
  return apiPost<RawGraphContext>("/graph/context", {
    seed,
    direction: options.direction,
    hops: options.hops,
    relationshipTypes:
      options.relationshipTypes && options.relationshipTypes.length > 0
        ? options.relationshipTypes
        : undefined,
  })
    .then(toGraphView)
    .then(resolveNodeTypes);
}

/** Colour, glyph and type caption all key off `semanticType`, and
 *  `/graph/context` does not return one — the route resolves each node's
 *  class internally to pick a label and deliberately keeps it out of its
 *  response shape. Asking SPARQL directly is what makes the canvas legible
 *  without changing that contract or teaching the console any pack's
 *  vocabulary.
 *
 *  **A failed type lookup degrades to an untyped picture, never to no
 *  picture.** Types are a reading aid; the neighbourhood is the answer, and
 *  losing the whole graph because a secondary query failed would trade the
 *  thing the reader asked for against a decoration. */
async function resolveNodeTypes(view: GraphView): Promise<GraphView> {
  const query = nodeTypeQuery(view.nodes.map((node) => node.id));
  if (query === null) return view;
  try {
    const result = await runSparql(query);
    return withNodeTypes(view, typesFromTypeRows(result.rows));
  } catch {
    return view;
  }
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

// ---- Paths (Plan 122a A4) — the asset-only `fetchLineage`/`LineageGraph`
// binding that used to sit here was removed along with the standalone
// `/lineage-view` page that was its only caller; Explore's own Entity tab
// gets real lineage-shaped data (upstream/downstream counts) from
// `fetchGraphContext`/`fetchAssetGraph` instead, which works for a
// graph-only subject too, not only a catalog asset. ----

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
  /** Who decided, once somebody has — `null` while `status` is `pending`. */
  readonly decidedBy?: string | null;
  /** Why they decided that. Required by the server on rejection. */
  readonly reason?: string | null;
  /** The rule's own rank against a pack's other rules — lower is more
   *  actionable. `undefined`/`null` when the rule declared none. */
  readonly priority?: number | null;
  readonly subjectLabel?: string | null;
}

export interface FindingsFilter {
  readonly pack?: string;
  readonly status?: Finding["status"];
}

export function fetchFindings(filter: FindingsFilter = {}): Promise<readonly Finding[]> {
  return apiFetch<readonly Finding[]>(`/findings${findingsQueryString(filter)}`);
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

// ---- Shapes: seed, preview, import — Plan 126, closing the "why is this
// all zero" gap: no UI previously existed to trigger any of this. ----

export interface ShapeFlake {
  readonly s: string;
  readonly p: string;
  readonly o: string;
  readonly t: number;
}

export interface RunValidationResult {
  readonly conforms: boolean;
  readonly violations: number;
  readonly warnings: number;
  readonly info: number;
  readonly shapes: number;
  readonly refusedShapes: number;
  readonly computedAtT: number;
  readonly sparqlTruncated: boolean;
}

export function runValidation(): Promise<RunValidationResult> {
  return apiPost<RunValidationResult>("/validation/runs", {});
}

export interface ShapesPreviewViolation {
  readonly shape: string;
  readonly focusNode: string;
  readonly path: string | null;
  readonly constraint: string;
  readonly severity: Severity;
  readonly message: string;
  readonly actual: string | null;
  readonly suggestion: unknown;
}

export interface ShapeConstraintDetail {
  readonly path: string | null;
  readonly kind: string;
  readonly detail: string;
}

export interface ShapeTargetDetail {
  readonly kind: string;
  readonly value: unknown;
}

/** A shape's own target/constraints in plain language — what "57 flakes
 *  written" could never say on its own. */
export interface ShapeDetail {
  readonly id: string;
  readonly target: ShapeTargetDetail;
  readonly severity: Severity;
  readonly message: string | null;
  readonly constraints: readonly ShapeConstraintDetail[];
}

/** Tagged the same way `ontology_editor_preview`'s three-response family
 *  already is (`kind`), so one reader handles `/seed` (the built-in set),
 *  `/preview` (nothing written) and `/import` (written for real) alike —
 *  they differ only in which shapes and whether anything was written,
 *  never in the shape of the answer. */
export type ShapesPreviewResult =
  | {
      readonly kind: "syntaxError";
      readonly message: string;
      readonly line: number | null;
      readonly column: number | null;
    }
  | {
      readonly kind: "checked";
      readonly shapes: number;
      readonly shapeDetails: readonly ShapeDetail[];
      readonly refusedShapes: readonly string[];
      readonly flakes: readonly ShapeFlake[];
      readonly conforms: boolean;
      readonly violations: number;
      readonly warnings: number;
      readonly info: number;
      readonly sample: readonly ShapesPreviewViolation[];
    };

export function seedCoreShapes(): Promise<ShapesPreviewResult> {
  return apiPost<ShapesPreviewResult>("/validation/shapes/seed", {});
}

export function previewShapes(document: string): Promise<ShapesPreviewResult> {
  return apiPost<ShapesPreviewResult>("/validation/shapes/preview", { format: "turtle", document });
}

export function importShapes(document: string): Promise<ShapesPreviewResult> {
  return apiPost<ShapesPreviewResult>("/validation/shapes/import", { format: "turtle", document });
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

/** Manually trigger entity resolution for one catalog asset —
 *  `POST /assets/{id}/resolve`, the same check that otherwise only ever
 *  runs automatically on streamed ingestion (`19-streaming.md` decision 7).
 *  Existed on the backend with no caller anywhere in the console until the
 *  Entity page wired it in — a business admin had no way to ask "does this
 *  already exist?" for something created directly through the console or a
 *  batch import. */
export function resolveAsset(id: string): Promise<Resolution> {
  return apiPost<Resolution>(`/assets/${encodeURIComponent(id)}/resolve`, {});
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

// ---- INGEST — Sources, Connectors (Plan 122a A6) ----

export interface ConnectorRun {
  readonly id: string;
  readonly connector: string;
  readonly serviceName: string;
  readonly startedAt: string;
  readonly finishedAt: string | null;
  readonly created: number;
  readonly skipped: number;
  readonly failed: number;
  readonly deleted: number;
  readonly refusal: string | null;
  readonly triggeredBy: string;
}

export function fetchConnectorRuns(
  filters: { readonly serviceName?: string; readonly limit?: number } = {},
): Promise<readonly ConnectorRun[]> {
  const params = new URLSearchParams();
  if (filters.serviceName) params.set("serviceName", filters.serviceName);
  params.set("limit", String(filters.limit ?? 200));
  return apiFetch<readonly ConnectorRun[]>(`/connectors/runs?${params.toString()}`);
}

export interface ConnectorConfigSchema {
  readonly properties: Readonly<Record<string, { readonly type: string; readonly title: string; readonly writeOnly?: boolean }>>;
  readonly required: readonly string[];
}

export function fetchConnectorSchema(connector: string): Promise<ConnectorConfigSchema> {
  return apiFetch<ConnectorConfigSchema>(`/connectors/${encodeURIComponent(connector)}/schema`);
}

export interface ConnectionTestResult {
  readonly ok: boolean;
  readonly detail?: string;
}

export function testConnector(
  connector: string,
  settings: Record<string, unknown>,
  secret?: string,
): Promise<ConnectionTestResult> {
  return apiPost<ConnectionTestResult>(`/connectors/${encodeURIComponent(connector)}/test`, { settings, secret });
}

export function runPostgresConnector(body: {
  readonly connectionString: string;
  readonly serviceName: string;
  readonly includeSchemas?: readonly string[];
}): Promise<ConnectorRun> {
  return apiPost<ConnectorRun>("/connectors/postgres/runs", body);
}

// ---- Vocabulary Studio (Plan 122a A7) ----

export interface Glossary {
  readonly id: string;
  readonly name: string;
  readonly description: string | null;
  readonly fullyQualifiedName: string;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export function fetchGlossaries(): Promise<readonly Glossary[]> {
  return apiFetch<readonly Glossary[]>("/glossaries");
}

export function createGlossary(name: string, description?: string): Promise<Glossary> {
  return apiPost<Glossary>("/glossaries", { name, description });
}

export function deleteGlossary(id: string, recursive = false): Promise<void> {
  return apiDelete(`/glossaries/${encodeURIComponent(id)}${recursive ? "?recursive=true" : ""}`);
}

export type TermStatus = "draft" | "inReview" | "approved" | "deprecated";

export interface GlossaryTerm {
  readonly id: string;
  readonly glossaryId: string;
  readonly name: string;
  readonly fullyQualifiedName: string;
  readonly definition: string;
  readonly status: TermStatus;
  readonly synonyms: readonly string[];
  readonly abbreviations: readonly string[];
  readonly version: string;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export function fetchGlossaryTerms(glossaryId: string): Promise<readonly GlossaryTerm[]> {
  return apiFetch<readonly GlossaryTerm[]>(`/glossaries/${encodeURIComponent(glossaryId)}/terms`);
}

export function fetchGlossaryTerm(id: string): Promise<GlossaryTerm> {
  return apiFetch<GlossaryTerm>(`/glossary-terms/${encodeURIComponent(id)}`);
}

export function createGlossaryTerm(
  glossaryId: string,
  body: { readonly name: string; readonly definition?: string; readonly synonyms?: readonly string[]; readonly abbreviations?: readonly string[] },
): Promise<GlossaryTerm> {
  return apiPost<GlossaryTerm>(`/glossaries/${encodeURIComponent(glossaryId)}/terms`, body);
}

export function updateGlossaryTerm(
  id: string,
  body: { readonly definition?: string; readonly synonyms?: readonly string[]; readonly abbreviations?: readonly string[] },
): Promise<GlossaryTerm> {
  return apiPatch<GlossaryTerm>(`/glossary-terms/${encodeURIComponent(id)}`, body);
}

export function deleteGlossaryTerm(id: string): Promise<void> {
  return apiDelete(`/glossary-terms/${encodeURIComponent(id)}`);
}

export function searchGlossaryTerms(q: string): Promise<readonly GlossaryTerm[]> {
  return apiFetch<readonly GlossaryTerm[]>(`/glossary-terms/search?q=${encodeURIComponent(q)}`);
}

export type SkosRelationKind = "broader" | "narrower" | "related" | "exactMatch" | "closeMatch";

export interface SkosRelation {
  readonly kind: SkosRelationKind;
  readonly target: string;
}

export function fetchTermRelations(id: string): Promise<readonly SkosRelation[]> {
  return apiFetch<readonly SkosRelation[]>(`/glossary-terms/${encodeURIComponent(id)}/relations`);
}

export function addTermRelation(id: string, kind: SkosRelationKind, target: string): Promise<SkosRelation> {
  return apiPost<SkosRelation>(`/glossary-terms/${encodeURIComponent(id)}/relations`, { kind, target });
}

export function deleteTermRelation(id: string, kind: SkosRelationKind, target: string): Promise<void> {
  const params = new URLSearchParams({ kind, target });
  return apiDelete(`/glossary-terms/${encodeURIComponent(id)}/relations?${params.toString()}`);
}

export function transitionTerm(
  id: string,
  to: TermStatus,
  reason?: string,
  successorTermId?: string,
): Promise<GlossaryTerm> {
  return apiPost<GlossaryTerm>(`/glossary-terms/${encodeURIComponent(id)}/transitions`, {
    to,
    reason,
    successorTermId,
  });
}

export function fetchTermUsage(id: string): Promise<{ readonly data: readonly string[]; readonly paging: { readonly after: string | null } }> {
  return apiFetch(`/glossary-terms/${encodeURIComponent(id)}/usage`);
}

export function fetchTermReviewers(id: string): Promise<{ readonly reviewers: readonly string[] }> {
  return apiFetch(`/glossary-terms/${encodeURIComponent(id)}/reviewers`);
}

export function setTermReviewers(id: string, reviewers: readonly string[]): Promise<{ readonly reviewers: readonly string[] }> {
  return apiPut(`/glossary-terms/${encodeURIComponent(id)}/reviewers`, { reviewers });
}

export interface SparqlResult {
  readonly rows: readonly Record<string, string>[];
  readonly factsScanned: number;
  readonly truncated: boolean;
  readonly asOf: number | null;
  readonly plan: readonly unknown[];
  readonly variables: readonly string[];
}

export function runSparql(query: string, asOf?: string): Promise<SparqlResult> {
  return apiPost<SparqlResult>("/sparql", { query, asOf });
}

// ---- Structural analytics (`src/lib/graph/analytics.ts`) — degree
// centrality, connected components and orphan detection over one bounded
// neighbourhood (`graph-owl-analytics`, Epic 38's `petgraph`-backed
// arithmetic), reachable at last from `GET /assets/{id}/analytics` and
// `POST /graph/context/analytics`. Never whole-graph: Epic 38's purity
// boundary forbids that on a synchronous request, so `truncated` on the
// response is load-bearing, not decorative. ----

export interface AssetAnalytics {
  readonly nodes: readonly string[];
  readonly inDegree: readonly number[];
  readonly outDegree: readonly number[];
  readonly orphans: readonly string[];
  readonly cycles: readonly (readonly string[])[];
  readonly edgeTypes: readonly string[];
  readonly truncated: boolean;
}

export function fetchAssetAnalytics(
  assetId: string,
  params: { hops?: number; maxNodes?: number } = {},
): Promise<AssetAnalytics> {
  const query = new URLSearchParams();
  if (params.hops !== undefined) query.set("hops", String(params.hops));
  if (params.maxNodes !== undefined) query.set("maxNodes", String(params.maxNodes));
  const suffix = query.toString();
  return apiFetch<AssetAnalytics>(`/assets/${encodeURIComponent(assetId)}/analytics${suffix ? `?${suffix}` : ""}`);
}

// ---- Ontology alignment review (`plans/ontology-alignment-review.md`) —
// Epic 104's cross-vocabulary alignment queue, wired for the first time.
// `GET /alignments/review` is never admin-gated (reviewing what is pending
// needs no elevated tier); `POST /alignments` — the Confirm/Reject action —
// is, matching every other admin-only graph write. ----

export interface Principal {
  readonly id: string;
  readonly name: string;
  readonly kind: "user" | "service" | "system";
  readonly roles: readonly string[];
  readonly isAdmin: boolean;
}

export function fetchWhoAmI(): Promise<Principal> {
  return apiFetch<Principal>("/me");
}

export interface AlignmentReviewEntry {
  readonly subject: string;
  readonly left: string | null;
  readonly right: string | null;
  readonly predicate: string | null;
  readonly sourceKind: string | null;
  readonly sourceDetail: string | null;
  readonly confidence: number | null;
  readonly lossyReverse: boolean | null;
}

export function fetchAlignmentReviewQueue(): Promise<readonly AlignmentReviewEntry[]> {
  return apiFetch<readonly AlignmentReviewEntry[]>("/alignments/review");
}

export interface UpsertAlignmentRequest {
  readonly kind: "match" | "equivalentClass";
  readonly left: string;
  readonly right: string;
  readonly predicate?: "exactMatch" | "closeMatch" | "broadMatch" | "narrowMatch" | "equivalentClass";
  readonly source: { readonly kind: "curated" | "computed" | "human"; readonly detail: string };
  readonly confidence: number;
  readonly lossyReverse: boolean;
}

export function upsertAlignment(request: UpsertAlignmentRequest): Promise<unknown> {
  return apiPost("/alignments", request);
}

export function fetchGraphContextAnalytics(body: {
  readonly seed: string;
  readonly direction?: "outgoing" | "incoming" | "both";
  readonly hops?: number;
  readonly maxNodes?: number;
  readonly relationshipTypes?: readonly string[];
  readonly asOf?: string | null;
}): Promise<AssetAnalytics> {
  return apiPost<AssetAnalytics>("/graph/context/analytics", body);
}

// ---- Ontology editor (`plans/ontology-editor.md`) — Epic 42 Slice G's
// already-shipped `/ontology-editor/{preview,dry-run,save}`, wired here for
// the first time. All three share one syntax-error shape, tagged `kind` to
// match the server's own internally-tagged enum exactly (`RdfEditDryRun`/
// `RdfEditSave` in `graph-owl-api`) — a client that pattern-matches `kind`
// handles all three responses the same way. ----

export interface OntologySyntaxError {
  readonly kind: "syntaxError";
  readonly message: string;
  readonly line: number | null;
  readonly column: number | null;
}

export interface OntologyPreviewTriple {
  readonly s: string;
  readonly p: string;
  readonly o: string;
  readonly oIsRef: boolean;
}

export interface OntologyEditPreview {
  readonly kind: "preview";
  readonly triples: readonly OntologyPreviewTriple[];
  readonly declared: readonly string[];
}

export type OntologyPreviewResult = OntologySyntaxError | OntologyEditPreview;

export interface OntologyEditChecked {
  readonly kind: "checked";
  readonly accepted: readonly string[];
  readonly rejected: readonly (readonly [string, string])[];
  readonly newInferences: number;
}

export type OntologyDryRunResult = OntologySyntaxError | OntologyEditChecked;

export interface OntologyEditSaved {
  readonly kind: "saved";
  readonly landed: readonly string[];
  readonly skipped: readonly string[];
  readonly rejected: readonly (readonly [string, string])[];
}

export type OntologySaveResult = OntologySyntaxError | OntologyEditSaved;

export function previewOntologyEdit(document: string): Promise<OntologyPreviewResult> {
  return apiPost<OntologyPreviewResult>("/ontology-editor/preview", { format: "turtle", document });
}

export function dryRunOntologyEdit(document: string): Promise<OntologyDryRunResult> {
  return apiPost<OntologyDryRunResult>("/ontology-editor/dry-run", { format: "turtle", document });
}

export function saveOntologyEdit(document: string): Promise<OntologySaveResult> {
  return apiPost<OntologySaveResult>("/ontology-editor/save", { format: "turtle", document });
}

// ---- Agents, MCP, Runs (Plan 122a A8 — real subset; see plans/122a-graphowl-app.md's A8.api gap note) ----

export type OwnerKind = "user" | "team";

export interface EntityReference {
  readonly id: string;
  readonly kind: OwnerKind;
  readonly displayName: string;
  readonly inherited: boolean;
}

export type AgentCapability =
  | "proposeDescription"
  | "proposeTags"
  | "proposeOwner"
  | "applyDescription"
  | "applyTags"
  | "recordMemory"
  | "recordInvestigation"
  | "createGlossaryTerm"
  | "createQualityTest"
  | "linkLineage";

export interface ScopeRef {
  readonly fqnPrefix: string;
}

export interface RateLimit {
  readonly maxWrites: number;
  readonly windowSeconds: number;
}

export interface AgentGrant {
  readonly id: string;
  readonly agent: EntityReference;
  readonly capabilities: readonly AgentCapability[];
  readonly scope: ScopeRef | null;
  readonly rateLimit: RateLimit;
  readonly expiresAt: string | null;
  readonly grantedBy: string;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export function fetchAgentGrants(): Promise<readonly AgentGrant[]> {
  return apiFetch<readonly AgentGrant[]>("/agents/grants");
}

export function setAgentGrant(
  agentId: string,
  body: {
    readonly capabilities: readonly AgentCapability[];
    readonly scopeFqnPrefix?: string;
    readonly maxWrites?: number;
    readonly windowSeconds?: number;
    readonly expiresAt?: string;
  },
): Promise<AgentGrant> {
  return apiPut<AgentGrant>(`/agents/${encodeURIComponent(agentId)}/grant`, body);
}

export function revokeAgentGrant(agentId: string): Promise<void> {
  return apiDelete(`/agents/${encodeURIComponent(agentId)}/grant`);
}

export type ActivityOutcome = "applied" | "proposed" | "refused";

export interface AgentActivity {
  readonly id: string;
  readonly agentId: string;
  readonly capability: AgentCapability;
  readonly targetFqn: string;
  readonly outcome: ActivityOutcome;
  readonly refusal: string | null;
  readonly at: string;
}

export function fetchAgentActivity(
  agentId: string,
  limit = 50,
): Promise<{ readonly data: readonly AgentActivity[]; readonly paging: { readonly after: string | null } }> {
  return apiFetch(`/agents/${encodeURIComponent(agentId)}/activity?limit=${limit}`);
}

export type ProposalStatus = "open" | "accepted" | "rejected" | "superseded";

export interface Proposal {
  readonly id: string;
  readonly proposedBy: EntityReference;
  readonly targetFqn: string;
  readonly capability: AgentCapability;
  readonly change: unknown;
  readonly rationale: string;
  readonly confidence: number;
  readonly status: ProposalStatus;
  readonly baseVersion: { readonly major: number; readonly minor: number };
  readonly decidedBy: string | null;
  readonly decidedAt: string | null;
  readonly createdAt: string;
}

export function fetchProposals(
  filters: { readonly agentId?: string; readonly status?: ProposalStatus; readonly limit?: number } = {},
): Promise<{ readonly data: readonly Proposal[]; readonly paging: { readonly after: string | null } }> {
  const params = new URLSearchParams();
  if (filters.agentId) params.set("agentId", filters.agentId);
  if (filters.status) params.set("status", filters.status);
  params.set("limit", String(filters.limit ?? 200));
  return apiFetch(`/proposals?${params.toString()}`);
}

export function acceptProposal(id: string): Promise<Proposal> {
  return apiPost<Proposal>(`/proposals/${encodeURIComponent(id)}/accept`, {});
}

export function rejectProposal(id: string): Promise<void> {
  return apiPost<void>(`/proposals/${encodeURIComponent(id)}/reject`, {});
}

export interface McpTool {
  readonly name: string;
  readonly description?: string;
}

/** No REST listing exists for MCP tools — `/mcp` is the raw JSON-RPC
 *  transport (Epic 14 + 32), so this calls the protocol's own `tools/list`
 *  method rather than inventing a second, non-standard listing endpoint. */
export async function fetchMcpTools(): Promise<readonly McpTool[]> {
  const response = await apiPost<{
    readonly result?: { readonly tools: readonly McpTool[] };
    readonly error?: { readonly message: string };
  }>("/mcp", { jsonrpc: "2.0", id: 1, method: "tools/list" });
  if (response.error) {
    throw new Error(response.error.message);
  }
  return response.result?.tools ?? [];
}

// ---- PLATFORM — Workbench, Packs, Admin (Plan 122a A10) ----

export function runCypher(query: string, asOf?: string): Promise<SparqlResult> {
  return apiPost<SparqlResult>("/cypher", { query, asOf });
}

/** Already-installed packs are filtered out server-side
 *  (`scan_available_packs`) — this list is genuinely "not yet installed",
 *  so there is no `installed` field to carry. */
export interface AvailablePack {
  readonly id: string;
  readonly description: string;
}

export function fetchAvailablePacks(): Promise<readonly AvailablePack[]> {
  return apiFetch<readonly AvailablePack[]>("/packs/available");
}

/** A *different* id space from `AvailablePack.id`. `/packs/available` scans
 *  `pack.toml` files on disk and returns string slugs ("gst"); terms,
 *  overrides and upgrade all key off the installed pack *record*'s own
 *  UUID (`GET /ontology-packs`) — found live when `/ontology-packs/{id}/terms`
 *  400'd on a slug passed where a UUID was expected. */
export interface InstalledPack {
  readonly id: string;
  readonly packId: string;
  readonly version: string;
  readonly licence: unknown;
  readonly sourceUrl: string;
  readonly glossaryId: string;
  readonly termCount: number;
  readonly importedAt: string;
}

export function fetchInstalledPacks(): Promise<readonly InstalledPack[]> {
  return apiFetch<readonly InstalledPack[]>("/ontology-packs");
}

export function installPack(pack: string): Promise<{ readonly pack: string; readonly ok: boolean; readonly output: string }> {
  return apiPost(`/packs/${encodeURIComponent(pack)}/install`, {});
}

export interface PackTermView {
  readonly sourceIri: string;
  readonly term: GlossaryTerm;
  readonly effective: boolean;
}

export function fetchPackTerms(packId: string): Promise<readonly PackTermView[]> {
  return apiFetch<readonly PackTermView[]>(`/ontology-packs/${encodeURIComponent(packId)}/terms`);
}

export type PackOverrideKind = "hide" | "relabel" | "reparent";

export interface PackOverride {
  readonly id: string;
  readonly termPath: string;
  readonly kind: PackOverrideKind;
  readonly payload: unknown;
}

export function fetchPackOverrides(packId: string): Promise<readonly PackOverride[]> {
  return apiFetch<readonly PackOverride[]>(`/ontology-packs/${encodeURIComponent(packId)}/overrides`);
}

export function createPackOverride(
  packId: string,
  body: { readonly termPath: string; readonly kind: PackOverrideKind; readonly payload?: unknown },
): Promise<PackOverride> {
  return apiPost<PackOverride>(`/ontology-packs/${encodeURIComponent(packId)}/overrides`, body);
}

export function deletePackOverride(packId: string, overrideId: string): Promise<void> {
  return apiDelete(`/ontology-packs/${encodeURIComponent(packId)}/overrides/${encodeURIComponent(overrideId)}`);
}

export interface PackUpgradeResult {
  readonly report: unknown;
  readonly applied: boolean;
}

export function upgradePack(packId: string, version: string, manifest: string, dryRun: boolean): Promise<PackUpgradeResult> {
  const params = new URLSearchParams({ version, dryRun: String(dryRun) });
  return fetch(`${API_BASE}/ontology-packs/${encodeURIComponent(packId)}/upgrade?${params.toString()}`, {
    method: "POST",
    headers: { "content-type": "text/plain" },
    body: manifest,
  }).then((response) => {
    if (!response.ok) throw new Error(`upgrade responded ${response.status}`);
    return response.json() as Promise<PackUpgradeResult>;
  });
}

export interface Team {
  readonly id: string;
  readonly parentTeamId: string | null;
  readonly displayName: string;
  readonly description: string | null;
  readonly members: readonly string[];
}

export function fetchTeams(): Promise<readonly Team[]> {
  return apiFetch<readonly Team[]>("/teams");
}

export function upsertTeam(team: {
  readonly id: string;
  readonly displayName: string;
  readonly description?: string;
  readonly members?: readonly string[];
  readonly parentTeamId?: string;
}): Promise<Team> {
  return apiPost<Team>("/teams", team);
}

export function deleteTeam(id: string): Promise<void> {
  return apiDelete(`/teams/${encodeURIComponent(id)}`);
}

export function upsertUser(id: string, displayName: string, email?: string): Promise<unknown> {
  return apiPut(`/users/${encodeURIComponent(id)}`, { displayName, email });
}

export function setUserRoles(id: string, roles: readonly string[]): Promise<unknown> {
  return apiPut(`/users/${encodeURIComponent(id)}/roles`, { roles });
}

export function fetchMapping(name: string): Promise<unknown> {
  return apiFetch(`/webhooks/mappings/${encodeURIComponent(name)}`);
}

export function fetchHealth(): Promise<{ readonly status: string; readonly version: string }> {
  return apiFetch("/health");
}
