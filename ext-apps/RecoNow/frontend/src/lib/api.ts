const API_BASE = "";

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  if (!response.ok) {
    throw new Error(`${init?.method ?? "GET"} ${path} -> ${response.status}`);
  }
  return response.json() as Promise<T>;
}

function apiPost<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, { method: "POST", body: JSON.stringify(body) });
}

export interface Client {
  readonly id: string;
  readonly name: string;
  readonly gstin: string;
  readonly state: string;
}

export function fetchClients(): Promise<readonly Client[]> {
  return apiFetch<Client[]>("/api/clients");
}

export function createClient(input: { name: string; gstin: string; state: string }): Promise<Client> {
  return apiPost<Client>("/api/clients", input);
}

export interface Period {
  readonly id: string;
  readonly month: string;
  readonly year: number;
  readonly status: string;
}

export function fetchPeriods(clientId: string): Promise<readonly Period[]> {
  return apiFetch<Period[]>(`/api/clients/${encodeURIComponent(clientId)}/periods`);
}

export function createPeriod(clientId: string, input: { month: string; year: number }): Promise<Period> {
  return apiPost<Period>(`/api/clients/${encodeURIComponent(clientId)}/periods`, input);
}

export interface AskResult {
  readonly grounded: boolean;
  readonly answer: string;
  readonly citations: readonly string[];
}

export function askQuestion(clientId: string, periodId: string, question: string): Promise<AskResult> {
  return apiPost<AskResult>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/ask`,
    { question },
  );
}

export interface Approval {
  readonly id: string;
  readonly decision_type: string;
  readonly amount: number | null;
  readonly status: string;
}

export function fetchApprovals(clientId: string, periodId: string, status = "pending"): Promise<readonly Approval[]> {
  return apiFetch<Approval[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/approvals?status=${status}`,
  );
}

/** A problem found in an uploaded file, reported when it lands rather than
 *  discovered later as a check that quietly did not run. */
export interface DataIssue {
  readonly code: string;
  readonly detail: string;
  readonly severity: "blocking" | "warning";
  /** How many rows are affected. */
  readonly rows: number;
  /** 1-based, matching what a spreadsheet shows. */
  readonly example_row: number;
}

export interface DatasetUploadResult {
  readonly issues?: readonly DataIssue[];
  readonly kind: string;
  readonly name?: string;
  readonly headers: readonly string[];
  readonly preview: readonly Record<string, unknown>[];
  /** The file's own rows, capped by `row_limit`. Present when a stored
   *  dataset is reopened; the upload response carries `preview` only. */
  readonly rows?: readonly Record<string, unknown>[];
  readonly row_limit?: number;
  readonly total_rows: number;
  readonly mapping: Record<string, number | null>;
  readonly from_template: boolean;
  readonly confirmed?: boolean;
}

export function uploadDataset(
  clientId: string,
  periodId: string,
  kind: string,
  file: File,
): Promise<DatasetUploadResult> {
  const form = new FormData();
  form.append("file", file);
  return fetch(
    `${API_BASE}/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/datasets/${kind}/upload`,
    { method: "POST", body: form },
  ).then((r) => {
    if (!r.ok) throw new Error(`upload ${kind} -> ${r.status}`);
    return r.json() as Promise<DatasetUploadResult>;
  });
}

export function confirmDatasetMapping(
  clientId: string,
  periodId: string,
  kind: string,
  mapping: Record<string, number | null>,
  tolerance: number,
): Promise<{ kind: string; confirmed: boolean }> {
  return apiPost(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/datasets/${kind}/mapping`,
    { mapping, tolerance },
  );
}

/** Reopen a file uploaded earlier in this period, with its mapping as it
 *  now stands. Without this, navigating away from Upload & map and back
 *  showed an empty prompt even though the file was still there. */
export function fetchDataset(
  clientId: string,
  periodId: string,
  kind: string,
): Promise<DatasetUploadResult> {
  return apiFetch<DatasetUploadResult>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/datasets/${kind}`,
  );
}

export interface DatasetSummary {
  readonly kind: string;
  readonly name: string;
  readonly total_rows: number;
  readonly confirmed: boolean;
}

export function fetchDatasets(clientId: string, periodId: string): Promise<readonly DatasetSummary[]> {
  return apiFetch<DatasetSummary[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/datasets`,
  );
}

export interface ReconcileResult {
  readonly ok: boolean;
  readonly evaluated?: number;
  readonly found?: number;
  readonly cases_created?: number;
  readonly error?: string;
}

export function runReconcile(clientId: string, periodId: string): Promise<ReconcileResult> {
  return apiPost(`/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/reconcile`, {});
}

export interface DashboardCase {
  readonly invoice_no: string;
  readonly reason_code: string | null;
  readonly supplier_name: string | null;
  readonly exposure: number;
  readonly status: string;
}

export interface DashboardDataset {
  readonly kind: string;
  readonly name: string;
  readonly total_rows: number;
  readonly confirmed: boolean;
}

export interface Dashboard {
  readonly period_label: string | null;
  readonly case_count: number;
  readonly total_exposure: number;
  readonly needs_decision: readonly DashboardCase[];
  readonly pending_approvals: number;
  readonly supplier_count: number;
  readonly invoice_count: number;
  /** null when no books file has been uploaded — distinct from 0. */
  readonly books_total: number | null;
  readonly clean_total: number | null;
  readonly datasets: readonly DashboardDataset[];
  readonly reconciled: boolean;
}

export function fetchDashboard(clientId: string, periodId: string): Promise<Dashboard> {
  return apiFetch<Dashboard>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/dashboard`,
  );
}

export interface RegisterRow {
  readonly id: string;
  readonly invoice_no: string;
  readonly reason_code: string | null;
  readonly status: string;
  readonly supplier_name: string | null;
  readonly supplier_gstin: string | null;
  readonly books_amount: number | null;
  readonly portal_amount: number | null;
  readonly exposure: number;
}

export interface Register {
  readonly rows: readonly RegisterRow[];
  readonly total_exposure: number;
}

export function fetchRegister(clientId: string, periodId: string, reasonCode?: string): Promise<Register> {
  const query = reasonCode ? `?reason_code=${encodeURIComponent(reasonCode)}` : "";
  return apiFetch<Register>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/register${query}`,
  );
}

export interface ExceptionGroup {
  readonly reason_code: string;
  readonly count: number;
  readonly total_exposure: number;
}

export function fetchExceptions(clientId: string, periodId: string): Promise<readonly ExceptionGroup[]> {
  return apiFetch<ExceptionGroup[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/exceptions`,
  );
}

export interface EvidenceFact {
  readonly predicate: string | null;
  readonly value: string | null;
  /** The variable the rule bound this fact to — `claimed` vs `filed` is what
   *  makes two identical predicates readable as two sides of a comparison. */
  readonly var: string | null;
}

export interface CaseDetail extends RegisterRow {
  readonly subject: string | null;
  readonly summary: string | null;
  readonly governed_by: string | null;
  readonly evidence_count: number | null;
  /** The facts behind this case, read live from graph-owl. Empty when
   *  graph-owl is unreachable — `graph_reachable` distinguishes that from a
   *  finding that genuinely cites nothing. */
  readonly evidence: readonly EvidenceFact[];
  readonly graph_reachable: boolean;
  readonly group_reason_code: string | null;
  readonly graphowl_url: string;
  readonly prev_id: string | null;
  readonly next_id: string | null;
  readonly ims_decisions: readonly { decision: string; decided_at: string }[];
}

export function fetchCaseDetail(clientId: string, periodId: string, caseId: string): Promise<CaseDetail> {
  return apiFetch<CaseDetail>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/register/${encodeURIComponent(caseId)}`,
  );
}

export function recordImsDecision(
  clientId: string,
  periodId: string,
  caseId: string,
  decision: "accept" | "reject" | "pending",
): Promise<{ id: string; decision: string }> {
  return apiPost(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/register/${encodeURIComponent(caseId)}/ims`,
    { decision },
  );
}

export function decideApproval(
  clientId: string,
  periodId: string,
  approvalId: string,
  status: "approved" | "rejected",
): Promise<Approval> {
  return apiPost<Approval>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/approvals/${encodeURIComponent(approvalId)}/decide`,
    { status },
  );
}

export interface SupplierSummary {
  readonly gstin: string;
  readonly name: string | null;
  readonly case_count: number;
  readonly total_exposure: number;
  readonly pending_count: number;
}

export function fetchSuppliers(clientId: string, periodId: string): Promise<readonly SupplierSummary[]> {
  return apiFetch<SupplierSummary[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/suppliers`,
  );
}

export interface ItcPosition {
  readonly position: Readonly<Record<string, number>>;
  readonly counts: Readonly<Record<string, number>>;
  readonly explain?: Readonly<Record<string, FigureExplanation>>;
  /** Why this screen's total legitimately differs from the working paper's. */
  readonly compare_note?: string;
}

export function fetchItcPosition(clientId: string, periodId: string): Promise<ItcPosition> {
  return apiFetch<ItcPosition>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/itc`,
  );
}

/** One line of the GSTR-3B working paper. `source` is what makes it a working
 *  paper rather than a summary: a figure a reviewer cannot trace is one they
 *  have to take on trust. `citation` is present only on statutory deductions. */
export interface WorkingPaperLine {
  readonly key: string;
  readonly kind: "opening" | "deduction" | "closing";
  readonly label: string;
  readonly amount: number;
  /** How many findings landed on this line with no amount anybody
   *  established. Counted, never coerced to zero — a deduction of zero and
   *  one of unknown size are different claims, and zeroing makes the net
   *  figure overstate what is claimable. */
  readonly unquantified: number;
  readonly source: string;
  readonly citation: string | null;
}

/** How the computed position compares to the return that was actually filed.
 *  Kept apart from the chain deliberately — netting a computed claim against a
 *  filed one hides which of the two is being asserted. */
export interface WorkingPaperFiled {
  readonly direction: "excess" | "unclaimed" | "agrees" | "not_evaluated";
  readonly difference: number | null;
  readonly needs: string | null;
  readonly available_2b: number;
  readonly gross_claimed: number | null;
  readonly reversed: number | null;
  readonly net_claimed: number | null;
  readonly arithmetic_ok: boolean | null;
}

export interface WorkingPaper {
  readonly lines: readonly WorkingPaperLine[];
  readonly explain?: Readonly<Record<string, FigureExplanation>>;
  /** Why this total legitimately differs from the ITC position screen's. */
  readonly compare_note?: string;
  /** Whether every deduction the chain names could be sized. A paper with an
   *  unquantified line is still the best available position; it just is not
   *  the final one. */
  readonly complete: boolean;
  /** Findings the chain has no line for. Surfaced rather than dropped: a
   *  deduction with nowhere to go would make the net figure overstate what is
   *  claimable, silently. */
  readonly unmodelled: readonly { readonly label: string; readonly amount: number }[];
  readonly filed: WorkingPaperFiled;
}

export function fetchWorkingPaper(clientId: string, periodId: string): Promise<WorkingPaper> {
  return apiFetch<WorkingPaper>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/working-paper`,
  );
}

export interface AtRiskSupplier {
  readonly gstin: string;
  readonly name: string | null;
  readonly at_risk_amount: number;
  readonly case_count: number;
}

export function fetchAtRisk(clientId: string, periodId: string): Promise<readonly AtRiskSupplier[]> {
  return apiFetch<AtRiskSupplier[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/atrisk`,
  );
}

export interface FollowUp {
  readonly case_id: string;
  readonly invoice_no: string;
  readonly supplier_name: string | null;
  readonly reason_code: string | null;
  readonly exposure: number;
  readonly status: string;
  readonly subject: string | null;
  readonly summary: string | null;
}

export function fetchFollowUps(clientId: string, periodId: string): Promise<readonly FollowUp[]> {
  return apiFetch<FollowUp[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/followups`,
  );
}

export interface AuthorityRow {
  readonly authority: string;
  readonly case_count: number;
  readonly exposure: number;
}

export function fetchAuthority(clientId: string, periodId: string): Promise<readonly AuthorityRow[]> {
  return apiFetch<AuthorityRow[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/authority`,
  );
}

export interface ObligationRow {
  readonly obligation: string;
  readonly case_count: number;
  readonly exposure: number;
}

export function fetchObligations(clientId: string, periodId: string): Promise<readonly ObligationRow[]> {
  return apiFetch<ObligationRow[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/obligations`,
  );
}

export interface RiskSupplier {
  readonly gstin: string;
  readonly name: string | null;
  readonly case_count: number;
  readonly total_exposure: number;
  readonly max_exposure: number;
  readonly pending_count: number;
}

export function fetchRisk(clientId: string, periodId: string): Promise<readonly RiskSupplier[]> {
  return apiFetch<RiskSupplier[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/risk`,
  );
}

export interface ResetStatus {
  readonly clients: number;
  readonly periods: number;
  readonly cases: number;
  readonly approvals: number;
  readonly users: number;
}

export function fetchResetStatus(): Promise<ResetStatus> {
  return apiFetch<ResetStatus>("/api/reset/status");
}

export interface Deliverable {
  readonly id: string;
  readonly kind: string;
  readonly status: string;
  readonly generated_at: string;
}

export function fetchDeliverables(clientId: string, periodId: string): Promise<readonly Deliverable[]> {
  return apiFetch<Deliverable[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/deliverables`,
  );
}

export interface ImportRecord {
  readonly id: string;
  readonly kind: string;
  readonly columns_mapped: number;
  readonly tolerance: number;
  readonly imported_at: string;
}

export function fetchImports(clientId: string, periodId: string): Promise<readonly ImportRecord[]> {
  return apiFetch<ImportRecord[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/imports`,
  );
}

export interface MappingRecord {
  readonly id: string;
  readonly dataset_kind: string;
  readonly mapping: Record<string, unknown>;
  readonly tolerance: number;
  readonly updated_at: string;
}

export function fetchMappings(clientId: string, periodId: string): Promise<readonly MappingRecord[]> {
  return apiFetch<MappingRecord[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/mappings`,
  );
}

export interface Rule {
  readonly id: string;
  readonly code: string;
  readonly name: string;
  readonly severity: string;
  readonly enabled: boolean;
  readonly case_count: number;
}

export function fetchRules(): Promise<readonly Rule[]> {
  return apiFetch<Rule[]>("/api/rules");
}

export interface User {
  readonly id: string;
  readonly name: string;
  readonly email: string;
  readonly role: string;
  readonly assigned_cases: number;
}

export function fetchUsers(): Promise<readonly User[]> {
  return apiFetch<User[]>("/api/users");
}

export interface CrossPeriodRow {
  readonly period: string;
  readonly period_id: string;
  readonly gstin: string;
  readonly name: string | null;
  readonly case_count: number;
  readonly exposure: number;
}

export function fetchCrossPeriod(clientId: string, periodId: string): Promise<readonly CrossPeriodRow[]> {
  return apiFetch<CrossPeriodRow[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/crossperiod`,
  );
}

export interface EligibilityRow {
  readonly gstin: string;
  readonly name: string | null;
  readonly invoice_no: string;
  readonly books_amount: number;
  readonly portal_amount: number;
  readonly eligibility: string;
}

export function fetchEligibility(clientId: string, periodId: string): Promise<readonly EligibilityRow[]> {
  return apiFetch<EligibilityRow[]>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/eligibility`,
  );
}

export interface GraphOwlStatus {
  readonly ok: boolean;
  readonly server: string;
  readonly reachable: boolean;
  readonly pack: { readonly id: string; readonly version: string; readonly terms: number } | null;
}

export function fetchGraphOwlStatus(): Promise<GraphOwlStatus> {
  return apiFetch<GraphOwlStatus>("/api/graphowl/status");
}

export interface PeriodPoint {
  readonly period_id: string;
  readonly label: string;
  readonly case_count: number;
  readonly exposure: number;
  readonly status: string;
}

export interface Analytics {
  readonly periods: readonly PeriodPoint[];
  /** False when there is one period or none. A single point is a number,
   *  not a trend, and the screen must not draw a line through it. */
  readonly has_trend: boolean;
}

export function fetchAnalytics(clientId: string): Promise<Analytics> {
  return apiFetch<Analytics>(`/api/clients/${encodeURIComponent(clientId)}/analytics`);
}

export type Bucket = "matched" | "review" | "only_books" | "only_portal";

export interface ReconRow {
  readonly invoice_no: string | null;
  readonly supplier_gstin: string | null;
  readonly supplier_name: string | null;
  readonly bucket: Bucket;
  readonly books_taxable: number;
  readonly portal_taxable: number;
  readonly books_tax: number;
  readonly portal_tax: number;
  readonly difference: number;
  readonly labels: readonly string[];
  readonly blocked: boolean;
}

export interface ItcPositionBreakdown {
  readonly confirmed: number;
  /** Deferred, not lost — the supplier has not filed yet. */
  readonly pending: number;
  /** s.17(5) or reverse charge. Lost. */
  readonly blocked: number;
  readonly under_review: number;
  /** On the portal, absent from the books. */
  readonly unclaimed: number;
  readonly total_considered: number;
}

/** Whether a rule reached a conclusion, as **graph-owl reported it**. Not
 *  inferred in the frontend: "could not evaluate" is execution evidence, and
 *  it belongs in the engine's record, not in a banner. */
export type RuleStatus = "passed" | "flagged" | "notEvaluated";

/** What a rule means in a business reader's terms, and what to do about it.
 *  Authored in `packs/gst`'s own `[findings.guidance]`, never here — a
 *  healthcare or banking pack names entirely different findings. */
export interface RuleGuidance {
  readonly title: string | null;
  readonly meaning: string | null;
  readonly next_action: string | null;
  readonly tone: string | null;
}

/** How a figure was derived, what it means, and what to do about it. */
export interface FigureExplanation {
  readonly means: string;
  readonly formula: string;
  readonly action: string;
  readonly source: string;
}

export interface RuleOutcome {
  /** The rule's authored human title. `gst:AmountMismatch` means nothing to a
   *  business reader; this does. Falls back to a readable phrase derived from
   *  the label when a pack has authored none. */
  readonly title?: string | null;
  readonly meaning?: string | null;
  readonly next_action?: string | null;
  readonly label: string;
  readonly governed_by: string | null;
  /** The rule's own one-line statement of what it looks for. A label alone
   *  tells a reviewer nothing. */
  readonly summary: string | null;
  readonly status: RuleStatus;
  readonly found: number;
  /** Classes or predicates the rule needed and did not find. Non-empty only
   *  when `status` is `notEvaluated`. */
  readonly unmet: readonly string[];
  readonly recorded_at: string;
}

export interface Reconciliation {
  /** How each figure on this screen was derived, what it means and what to do.
   *  Sent with the data so a figure and its stated derivation are one edit. */
  readonly explain?: Readonly<Record<string, FigureExplanation>>;
  /** What each rule concluded on the last run, from the engine's own
   *  execution record. Empty before a reconciliation has been run. */
  readonly rule_outcomes: readonly RuleOutcome[];
  /** Rule label -> why it matters, for checks the uploaded files cannot
   *  support. The pre-reconciliation view: before a run there are no
   *  outcomes, and a reviewer still needs to know what is unsupported. */
  readonly checks_disabled: Record<string, string>;
  readonly total: number;
  readonly match_rate: number;
  readonly counts: Record<Bucket, number>;
  readonly itc: ItcPositionBreakdown;
  readonly have_books: boolean;
  readonly have_portal: boolean;
  readonly rows: readonly ReconRow[];
}

export function fetchReconciliation(clientId: string, periodId: string): Promise<Reconciliation> {
  return apiFetch<Reconciliation>(
    `/api/clients/${encodeURIComponent(clientId)}/periods/${encodeURIComponent(periodId)}/reconciliation`,
  );
}
