/** Plan 122b B1's own RED, made a pure, testable state transition rather
 *  than something only checkable by clicking through the UI: "switching
 *  client clears the case list rather than showing stale rows." A case
 *  list is always fetched scoped to (clientId, periodId) — so the actual
 *  mechanism that prevents a stale list is that switching the client must
 *  also clear the selected period, which every period-scoped fetch depends
 *  on. A mutant that keeps the old periodId after a client switch is
 *  exactly what `workspace.test.ts` exists to catch. */

export interface WorkspaceState {
  readonly clientId: string | null;
  readonly periodId: string | null;
}

export const EMPTY_WORKSPACE: WorkspaceState = { clientId: null, periodId: null };

export function selectClient(state: WorkspaceState, clientId: string): WorkspaceState {
  if (state.clientId === clientId) return state;
  return { clientId, periodId: null };
}

export function selectPeriod(state: WorkspaceState, periodId: string): WorkspaceState {
  return { ...state, periodId };
}

const STORAGE_KEY = "reconow.workspace";

export function persistWorkspace(state: WorkspaceState): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export function loadWorkspace(): WorkspaceState {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return EMPTY_WORKSPACE;
  try {
    const parsed = JSON.parse(raw) as Partial<WorkspaceState>;
    return {
      clientId: typeof parsed.clientId === "string" ? parsed.clientId : null,
      periodId: typeof parsed.periodId === "string" ? parsed.periodId : null,
    };
  } catch {
    return EMPTY_WORKSPACE;
  }
}
