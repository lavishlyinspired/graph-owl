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
