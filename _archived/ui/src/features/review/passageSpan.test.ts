import { describe, expect, it } from "vitest";
import { splitPassage } from "./passageSpan";

describe("splitPassage", () => {
  it("splits a passage into the text before, inside, and after the span", () => {
    const result = splitPassage("The invoice total exceeds the budget.", [4, 17]);

    expect(result).toEqual({
      before: "The ",
      highlighted: "invoice total",
      after: " exceeds the budget.",
    });
  });

  it("highlights the whole passage when the span covers it entirely", () => {
    const result = splitPassage("Revenue grew.", [0, 13]);

    expect(result).toEqual({ before: "", highlighted: "Revenue grew.", after: "" });
  });

  it("highlights nothing when the span is empty, at the start", () => {
    const result = splitPassage("Revenue grew.", [0, 0]);

    expect(result).toEqual({ before: "", highlighted: "", after: "Revenue grew." });
  });

  it("does not include the character at the end offset — end is exclusive", () => {
    const result = splitPassage("abcdef", [1, 3]);

    expect(result.highlighted).toBe("bc");
    expect(result.after).toBe("def");
  });

  it("clamps a span that runs past the end of the passage rather than throwing", () => {
    const result = splitPassage("short", [2, 999]);

    expect(result).toEqual({ before: "sh", highlighted: "ort", after: "" });
  });

  it("clamps a negative start rather than throwing", () => {
    const result = splitPassage("short", [-5, 3]);

    expect(result).toEqual({ before: "", highlighted: "sho", after: "rt" });
  });
});
