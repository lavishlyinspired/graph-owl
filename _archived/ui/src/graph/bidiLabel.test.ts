import { describe, expect, it } from "vitest";
import { canvasLabel } from "./bidiLabel";

// A four-letter Hebrew word ("peace") used only as a right-to-left fixture —
// the tests below depend on it being a *single, uniform-direction* run with
// no digits or brackets, not on its meaning.
const HEBREW_WORD = "שלום";
const ARABIC_WORD = "مرحبا";

describe("canvasLabel", () => {
  it("leaves a label with no right-to-left characters unchanged — the common case for this catalog's own data", () => {
    expect(canvasLabel("public.orders.customer_id")).toBe(
      "public.orders.customer_id",
    );
    expect(canvasLabel("")).toBe("");
  });

  /** The one case this module's own doc comment records as verified
   *  against a real, running browser — not just against `bidi-js`'s own
   *  output. Rendered via Cytoscape's actual `fillText` call, read back
   *  with `getImageData` and cross-correlated against individually
   *  rendered reference glyphs (not eyeballed — see the module doc
   *  comment for why that specifically was not trustworthy here), and
   *  found to match the DOM's own `dir="auto"` rendering of the same
   *  logical string pixel-for-pixel in letter identity and order. */
  it("moves a trailing left-to-right run before a leading right-to-left one, without reversing either run's own characters", () => {
    const input = `${HEBREW_WORD}_orders`;
    expect(canvasLabel(input)).toBe(`orders_${HEBREW_WORD}`);
  });

  /** `fillText` already reverses a *standalone* right-to-left run into
   *  correct reading order on its own (verified the same way as the case
   *  above) — regardless of the character order it is given. So the
   *  correct string to hand it is the *original*, untouched one: nothing
   *  here needs correcting, and "correcting" it anyway would only cause
   *  `fillText` to un-reverse a run that was already right. */
  it("leaves a standalone right-to-left label untouched — fillText reverses it correctly on its own", () => {
    expect(canvasLabel(HEBREW_WORD)).toBe(HEBREW_WORD);
  });

  it("does the same for a standalone Arabic label", () => {
    expect(canvasLabel(ARABIC_WORD)).toBe(ARABIC_WORD);
  });

  /** Same reasoning as the standalone case: the base direction here is
   *  left-to-right (the string starts with `a`), so there is no run to
   *  reposition — only to leave for `fillText`'s own correction. */
  it("leaves a right-to-left run embedded in left-to-right text untouched, when no repositioning is needed", () => {
    const input = `abc${HEBREW_WORD}def`;
    expect(canvasLabel(input)).toBe(input);
  });

  it("keeps a run of digits in their own left-to-right order when it gets repositioned", () => {
    const withDigits = `${HEBREW_WORD}123`;
    const result = canvasLabel(withDigits);
    expect(result).toContain("123");
    expect(result).not.toContain("321");
  });

  it("is a no-op for a string with only neutral or ascii characters even if it contains brackets or digits", () => {
    expect(canvasLabel("orders(2024)")).toBe("orders(2024)");
  });
});
