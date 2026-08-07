import { describe, expect, it } from "vitest";
import { performAsAction } from "./apiAction";
import { ApiError, type Problem } from "../../api";

function problem(overrides: Partial<Problem> = {}): Problem {
  return {
    type: "https://graph-owl.dev/errors/example",
    title: "Example problem",
    status: 400,
    detail: "something went wrong",
    ...overrides,
  };
}

describe("performAsAction", () => {
  it("resolves conflict: false when the action succeeds", async () => {
    const result = await performAsAction(async () => {
      /* no-op */
    });

    expect(result).toEqual({ conflict: false });
  });

  it("resolves conflict: true, rather than throwing, on a 409 ApiError", async () => {
    const result = await performAsAction(async () => {
      throw new ApiError(problem({ status: 409, title: "Already decided" }));
    });

    expect(result).toEqual({ conflict: true });
  });

  it("does not treat a non-409 status as a conflict", async () => {
    await expect(
      performAsAction(async () => {
        throw new ApiError(problem({ status: 400 }));
      }),
    ).rejects.toThrow();
  });

  it("flattens a non-conflict ApiError into a plain Error carrying the server's detail", async () => {
    await expect(
      performAsAction(async () => {
        throw new ApiError(problem({ status: 422, detail: "the reason was empty" }));
      }),
    ).rejects.toThrow("the reason was empty");
  });

  it("carries the problem's detail even when the title differs", async () => {
    await expect(
      performAsAction(async () => {
        throw new ApiError(problem({ status: 500, title: "Internal error", detail: "the write failed" }));
      }),
    ).rejects.toThrow("the write failed");
  });

  it("rethrows a non-ApiError failure unchanged", async () => {
    const original = new TypeError("network down");

    await expect(
      performAsAction(async () => {
        throw original;
      }),
    ).rejects.toBe(original);
  });
});
