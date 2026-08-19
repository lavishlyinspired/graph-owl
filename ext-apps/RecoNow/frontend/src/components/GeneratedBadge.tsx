/** Marks text an inference model wrote, everywhere it appears.
 *
 *  **One component, used by every surface that can show generated text.** A
 *  reader deciding how far to trust a sentence needs to know what produced it,
 *  and that judgement must not depend on which screen they happen to be on —
 *  two screens labelling the same thing differently is worse than neither
 *  labelling it.
 *
 *  Colour carries the distinction and is never the only thing that does: the
 *  word is there too, and the title says what it means. Colour alone fails for
 *  a colourblind reader and disappears in a printed working paper, which is
 *  exactly the document this product exists to produce. */
export function GeneratedBadge({
  source,
  grounded = true,
}: {
  readonly source: "model" | "computed";
  readonly grounded?: boolean;
}) {
  if (source === "computed") {
    return (
      <span
        className="rounded bg-reco-line px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider text-reco-t4"
        title="Computed directly from your data. No model was involved, and every figure is read from your rows."
      >
        computed
      </span>
    );
  }

  return (
    <span
      className="rounded bg-amber-100 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider text-amber-800"
      title={
        grounded
          ? "Written by an inference model from your own data. Every figure in it was checked against your rows — a figure your data does not carry would have been refused."
          : "Written by an inference model. Its figures were NOT verified."
      }
    >
      AI generated
    </span>
  );
}
