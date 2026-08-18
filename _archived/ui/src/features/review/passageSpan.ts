/** Epic 42 Slice D's own named RED test: a reviewer approving "this
 *  document says X" without seeing *where* it says it is guessing, and a
 *  reviewer who is guessing approves everything. `span` is already
 *  rebased onto `passage` server-side (`windowed_passage`,
 *  `graph-owl-api`'s extraction module) — this is the one remaining step,
 *  slicing the three parts a highlight needs to render. Pulled out as a
 *  pure function for the same reason `compareAssets`/`buildVocabularyTree`
 *  are: the decision a screen renders but does not make. */

export interface PassageSplit {
  readonly before: string;
  readonly highlighted: string;
  readonly after: string;
}

export function splitPassage(passage: string, span: readonly [number, number]): PassageSplit {
  const start = Math.max(0, Math.min(span[0], passage.length));
  const end = Math.max(start, Math.min(span[1], passage.length));
  return {
    before: passage.slice(0, start),
    highlighted: passage.slice(start, end),
    after: passage.slice(end),
  };
}
