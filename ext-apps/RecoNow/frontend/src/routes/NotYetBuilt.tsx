interface NotYetBuiltProps {
  readonly title: string;
  readonly slice: string;
}

/** Deliberately honest, not a finished screen — same reasoning as
 *  `graphowl-app/src/routes/PlaceholderPage.tsx`: says outright which
 *  slice builds it, so it cannot be mistaken for a shipped, empty one. */
export function NotYetBuilt({ title, slice }: NotYetBuiltProps) {
  return (
    <div className="p-8">
      <h1 className="text-[18px] font-semibold text-reco-t1">{title}</h1>
      <p className="mt-2 text-[13px] text-reco-t5">{`Ships in plans/122b-reconow-app.md, slice ${slice}.`}</p>
    </div>
  );
}
