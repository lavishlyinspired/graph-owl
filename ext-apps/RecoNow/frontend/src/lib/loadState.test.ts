import { describe, expect, it } from "vitest";
import { loadStateFor } from "./loadState";

describe("loadStateFor", () => {
  it("says a workspace is needed before it says anything is loading", () => {
    // Found live: /workingpaper rendered "Loading…" forever with no client
    // selected, because the fetch effect returned early and never cleared the
    // flag. A screen that says "loading" while waiting for a choice nobody has
    // been asked to make is a screen that looks broken.
    expect(loadStateFor({ clientId: "", periodId: "", loading: true, data: null })).toBe(
      "no-workspace",
    );
    expect(loadStateFor({ clientId: "c", periodId: "", loading: true, data: null })).toBe(
      "no-workspace",
    );
  });

  it("reports loading only once a workspace is chosen", () => {
    expect(loadStateFor({ clientId: "c", periodId: "p", loading: true, data: null })).toBe(
      "loading",
    );
  });

  it("distinguishes a finished fetch that found nothing from one still running", () => {
    // "No data for this period" and "still asking" are different answers, and
    // only one of them means the user should do something.
    expect(loadStateFor({ clientId: "c", periodId: "p", loading: false, data: null })).toBe(
      "empty",
    );
  });

  it("is ready when data arrived", () => {
    expect(loadStateFor({ clientId: "c", periodId: "p", loading: false, data: {} })).toBe(
      "ready",
    );
  });

  it("treats data that arrived while still loading as ready", () => {
    // A refresh over existing data must not blank the screen — the previous
    // answer is better than a spinner while a newer one is fetched.
    expect(loadStateFor({ clientId: "c", periodId: "p", loading: true, data: {} })).toBe(
      "ready",
    );
  });

  it("never reports ready without a workspace, whatever stale data is held", () => {
    // Switching client must not leave the previous client's figures on screen.
    expect(loadStateFor({ clientId: "", periodId: "", loading: false, data: {} })).toBe(
      "no-workspace",
    );
  });
});
