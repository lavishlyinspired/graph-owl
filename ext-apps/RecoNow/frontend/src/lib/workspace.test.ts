import { describe, expect, it } from "vitest";
import { EMPTY_WORKSPACE, selectClient, selectPeriod, type WorkspaceState } from "./workspace";

describe("selectClient", () => {
  it("switching to a different client clears the selected period", () => {
    const withPeriod: WorkspaceState = { clientId: "client-a", periodId: "period-aug" };
    const next = selectClient(withPeriod, "client-b");
    expect(next.clientId).toBe("client-b");
    expect(next.periodId).toBeNull();
  });

  it("re-selecting the already-current client leaves the period alone", () => {
    const withPeriod: WorkspaceState = { clientId: "client-a", periodId: "period-aug" };
    const next = selectClient(withPeriod, "client-a");
    expect(next.periodId).toBe("period-aug");
  });

  it("selecting a client from empty state starts with no period", () => {
    const next = selectClient(EMPTY_WORKSPACE, "client-a");
    expect(next).toEqual({ clientId: "client-a", periodId: null });
  });
});

describe("selectPeriod", () => {
  it("sets the period without touching the client", () => {
    const state: WorkspaceState = { clientId: "client-a", periodId: null };
    const next = selectPeriod(state, "period-aug");
    expect(next).toEqual({ clientId: "client-a", periodId: "period-aug" });
  });
});
