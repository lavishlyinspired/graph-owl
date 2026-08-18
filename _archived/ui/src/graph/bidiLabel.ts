/** Canvas-safe bidi text shaping — Epic 94 Slice C's own recorded gap.
 *
 *  `dir="auto"` (`../trust/direction.ts`) only ever reaches DOM text. Both
 *  the Explorer's graph (Epic 40) and the ontology editor's graph pane
 *  (Epic 42 Slice G) draw every node label onto a `<canvas>` through
 *  Cytoscape, and HTML's `dir` attribute has no meaning there.
 *
 *  **What Canvas2D's `fillText` actually does with a right-to-left run was
 *  not documented anywhere this project could find, and had to be
 *  discovered empirically against a real, running browser — twice, because
 *  the first empirical result was also read wrong.** The naive assumption
 *  (fillText draws exactly the codepoints it is given, left to right, with
 *  zero bidi awareness) is **false**: verified by rendering the untouched
 *  logical string `"לקוח_orders"` with a plain, unconfigured `fillText` and
 *  reading back the actual pixels (via `getImageData`, cross-correlated
 *  against individually-rendered reference glyphs — not by eye; eyeballing
 *  unfamiliar Hebrew letterforms across several attempts produced three
 *  different wrong answers in a row before this file settled on measuring
 *  instead of looking). The result: `fillText` **already reverses a run of
 *  strongly-right-to-left characters into correct reading order on its
 *  own**, regardless of the order the caller supplies them in. What it does
 *  *not* do is reposition whole runs relative to each other the way a
 *  right-to-left *paragraph* base direction requires (`dir="auto"` moves a
 *  trailing left-to-right run before a leading Hebrew/Arabic one; plain
 *  `fillText` leaves every run exactly where it was typed).
 *
 *  So the fix is *not* "reverse the whole string into visual order" (that
 *  double-reverses every right-to-left run — once here, once again inside
 *  `fillText` — landing back on the *wrong* reading order, confirmed by
 *  rendering the result and reading it the same measured way). It is:
 *  compute the fully-correct visual-order string with `bidi-js` (as if for
 *  a renderer with zero bidi awareness of its own), then reverse each
 *  right-to-left run *within that result* a second time — undoing the one
 *  correction `fillText` was always going to make for us, and keeping only
 *  the cross-run repositioning it does not.
 */

import bidiFactory from "bidi-js";

const bidi = bidiFactory();

/** Hebrew (U+0590-05FF), Arabic (U+0600-06FF), Hebrew presentation forms
 *  (U+FB1D-FB4F) and Arabic presentation forms A/B (U+FB50-FDFF,
 *  U+FE70-FEFF) — the standard Unicode RTL block ranges, written as
 *  escapes rather than pasted characters so the exact codepoints are
 *  auditable in a diff. A cheap pre-check so the common case (every label
 *  in this catalog's own fixtures, and most real ones) never touches
 *  bidi-js at all. Scoped to what the plan's own acceptance criterion
 *  names ("an Arabic or Hebrew label"), not every RTL script in Unicode. */
const RTL_CHARACTER = /[\u0590-\u06FF\uFB1D-\uFDFF\uFE70-\uFEFF]/;

/** Same ranges as `RTL_CHARACTER`, greedily matching a whole run rather
 *  than one character, and global so every run in the string is found. */
const RTL_RUN = /[\u0590-\u06FF\uFB1D-\uFDFF\uFE70-\uFEFF]+/g;

/** Reorders `text` into the string a Canvas2D `fillText` call must be given
 *  to render it in correct reading order — see the module doc comment for
 *  why this is not simply "the fully bidi-reordered string". A label with
 *  no right-to-left characters is returned unchanged.
 *
 *  **Known gap, recorded rather than silently assumed covered**: bracket
 *  mirroring (`(` naturally becoming `)` when its position flips inside a
 *  right-to-left run) is not attempted. A parenthesis in an otherwise
 *  right-to-left label may not mirror correctly; this was descoped after
 *  the empirical work above rather than shipped unverified — real asset
 *  and term names carrying a bracket *and* right-to-left text are rare
 *  enough that this is a documented limitation, not a silent one. */
export function canvasLabel(text: string): string {
  // Both early returns below are pure performance guards, not correctness
  // branches — mutation-tested and confirmed: removing either one still
  // produces identical output for every input, because running the full
  // algorithm on text with no right-to-left run is itself a correct no-op.
  // Kept anyway so the common case (most labels in this catalog's own
  // data) never allocates the intermediate arrays below.
  if (!text || !RTL_CHARACTER.test(text)) return text;

  const embeddingLevels = bidi.getEmbeddingLevels(text);
  const flips = bidi.getReorderSegments(text, embeddingLevels);
  if (flips.length === 0) return text;

  const chars = text.split("");
  // `.slice().reverse()` plus `.splice(...)` rather than a manual two-pointer
  // swap: both `start` and `end` are provably in bounds (bidi-js only ever
  // returns ranges within the string it was given), but expressing the swap
  // as direct `chars[i]`/`chars[j]` reads would need a non-null assertion at
  // every access under this project's `noUncheckedIndexedAccess`. Array
  // methods sidestep that without asserting anything.
  for (const [start, end] of flips) {
    const segment = chars.slice(start, end + 1).reverse();
    chars.splice(start, segment.length, ...segment);
  }

  // Undo the one correction `fillText` makes on its own — see the module
  // doc comment. Matched by script (not by the original flip ranges),
  // because that is the same signal `fillText` itself goes by.
  return chars.join("").replace(RTL_RUN, (run) => run.split("").reverse().join(""));
}
