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

export interface DatasetUploadResult {
  readonly kind: string;
  readonly headers: readonly string[];
  readonly preview: readonly Record<string, unknown>[];
  readonly total_rows: number;
  readonly mapping: Record<string, number | null>;
  readonly from_template: boolean;
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
