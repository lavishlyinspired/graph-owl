/** The base text direction for a user-supplied label — Epic 39 Slice E,
 *  from Epic 94 Slice C's `rdf:dirLangString`.
 *
 *  The store does not yet thread a stored base-direction flag through to
 *  the API, so this is not a lookup — it is the single named constant every
 *  render site imports instead of writing `dir="ltr"` by hand. `auto` asks
 *  the browser's own bidi algorithm to read the label's first strong
 *  character, which renders Arabic and Hebrew text correctly today and
 *  keeps being correct if a real per-label direction field arrives later,
 *  because every call site already goes through this one function. */
export function userTextDir(_text: string | null | undefined): "auto" {
  return "auto";
}
